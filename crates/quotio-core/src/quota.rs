//! Real provider quota fetching, ported from the quotio-master Swift fetchers.
//!
//! Each provider reads its CLIProxyAPI auth files under `~/.cli-proxy-api/` and
//! calls that provider's usage endpoint directly over HTTPS (routed through the
//! user's proxy, like the original app's proxied URLSession). Access tokens are
//! refreshed in-memory on 401 — we never write back to the auth files, since an
//! external CLIProxyAPI process may be using them concurrently.
//!
//! Ported so far: Codex/OpenAI (`OpenAIQuotaFetcher`), Claude Code
//! (`ClaudeCodeQuotaFetcher`).

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use base64::Engine;
use chrono::DateTime;
use quotio_types::{AccountQuota, QuotaModelUsage};
use serde::Deserialize;
use sha2::{Digest, Sha256};

// ---- Codex / OpenAI ----
const CODEX_USAGE_URL: &str = "https://chatgpt.com/backend-api/wham/usage";
const CODEX_TOKEN_REFRESH_URL: &str = "https://token.oaifree.com/api/auth/refresh";
// Spending one "主动重置次数" (rate-limit reset credit) force-resets the 5h
// primary window. Same endpoint + payload the CLIProxyAPI Management Center uses.
const CODEX_RESET_CREDITS_URL: &str =
    "https://chatgpt.com/backend-api/wham/rate-limit-reset-credits/consume";
// Codex CLI's User-Agent. The reset-credits endpoint is a write and stricter than
// the read-only usage endpoint, so we mirror what the official client sends.
const CODEX_USER_AGENT: &str = "codex_cli_rs/0.76.0 (Debian 13.0.0; x86_64) WindowsTerminal";

// ---- Claude Code ----
const CLAUDE_USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage";
const CLAUDE_TOKEN_URL: &str = "https://console.anthropic.com/v1/oauth/token";
const CLAUDE_CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";

// ---- GitHub Copilot ----
const COPILOT_ENTITLEMENT_URL: &str = "https://api.github.com/copilot_internal/user";

// ---- Antigravity (Google Cloud Code) ----
const ANTIGRAVITY_MODELS_URL: &str =
    "https://cloudcode-pa.googleapis.com/v1internal:fetchAvailableModels";
const ANTIGRAVITY_PROJECT_URL: &str =
    "https://cloudcode-pa.googleapis.com/v1internal:loadCodeAssist";
const ANTIGRAVITY_TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
// Antigravity uses Google's well-known installed-app OAuth credential. Not
// hardcoded in source — supplied at build time via env vars
// QUOTIO_ANTIGRAVITY_CLIENT_ID / QUOTIO_ANTIGRAVITY_CLIENT_SECRET (baked in by
// `option_env!`). Absent at build → Antigravity quota refresh is unavailable.
const ANTIGRAVITY_CLIENT_ID: &str = match option_env!("QUOTIO_ANTIGRAVITY_CLIENT_ID") {
    Some(v) => v,
    None => "",
};
const ANTIGRAVITY_CLIENT_SECRET: &str = match option_env!("QUOTIO_ANTIGRAVITY_CLIENT_SECRET") {
    Some(v) => v,
    None => "",
};
const ANTIGRAVITY_USER_AGENT: &str = "antigravity/1.11.3 Darwin/arm64";

// ---- Kiro (AWS CodeWhisperer) ----
const KIRO_VERSION: &str = "0.10.32";
const KIRO_DEFAULT_REGION: &str = "us-east-1";
const KIRO_REFRESH_ENDPOINT: &str = "https://prod.us-east-1.auth.desktop.kiro.dev/refreshToken";

// ---- GLM (BigModel) ----
const GLM_QUOTA_URL: &str = "https://bigmodel.cn/api/monitor/usage/quota/limit";

/// Fetch real quotas for every supported provider. Best-effort: a failure in
/// one provider or account never blocks the others.
///
/// `proxy_url` is the upstream proxy the user configured in Settings; provider
/// requests route through it (mirroring the macOS reference app), falling back
/// to the OS proxy env vars when it is empty.
pub fn fetch_all_quotas(proxy_url: Option<&str>) -> Vec<AccountQuota> {
    fetch_all_quotas_streaming(proxy_url, &|_| {})
}

/// Like [`fetch_all_quotas`], but invokes `emit` for each account the moment it
/// is fetched, so the UI can stream accounts in one-by-one instead of waiting
/// for the whole batch — and one unreachable account never blocks the display.
pub fn fetch_all_quotas_streaming(
    proxy_url: Option<&str>,
    emit: &(dyn Fn(&AccountQuota) + Sync),
) -> Vec<AccountQuota> {
    let agent = build_agent(proxy_url);
    let mut quotas = Vec::new();
    // Multi-account providers fetch concurrently and stream each account.
    quotas.extend(fetch_codex_quotas(&agent, emit));
    quotas.extend(fetch_gemini_quotas(&agent, emit));
    // Single / low-volume providers: fetch sequentially, then stream each.
    for account in fetch_claude_quotas(&agent) {
        emit(&account);
        quotas.push(account);
    }
    for account in fetch_copilot_quotas(&agent) {
        emit(&account);
        quotas.push(account);
    }
    for account in fetch_antigravity_quotas(&agent) {
        emit(&account);
        quotas.push(account);
    }
    for account in fetch_kiro_quotas(&agent) {
        emit(&account);
        quotas.push(account);
    }
    for account in fetch_glm_quotas(&agent) {
        emit(&account);
        quotas.push(account);
    }
    for account in fetch_trae_quotas(&agent) {
        emit(&account);
        quotas.push(account);
    }
    for account in fetch_cursor_quotas(&agent) {
        emit(&account);
        quotas.push(account);
    }
    quotas
}

/// Format an epoch reset time (seconds or milliseconds) as a relative label.
fn format_reset_epoch(value: f64) -> Option<String> {
    if value <= 0.0 {
        return None;
    }
    let secs = if value > 1.0e12 {
        (value / 1000.0) as i64
    } else {
        value as i64
    };
    format_reset_unix(secs)
}

// ===================== Shared helpers =====================

fn auth_dir() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))?;
    Some(PathBuf::from(home).join(".cli-proxy-api"))
}

/// Issue a request, retrying on transient transport errors (timeout / connection
/// reset — i.e. network jitter or a flaky upstream proxy) with a short backoff.
/// A real HTTP status (401/403/429/5xx) is returned immediately — that's a server
/// answer, not jitter, so retrying would only waste time. `build_request` is
/// re-invoked each attempt because `.call()` consumes the request.
fn call_with_retry(
    build_request: impl Fn() -> ureq::Request,
) -> Result<ureq::Response, ureq::Error> {
    let mut delay = Duration::from_millis(700);
    for attempt in 0..3u8 {
        match build_request().call() {
            Ok(response) => return Ok(response),
            Err(error @ ureq::Error::Status(_, _)) => return Err(error),
            Err(error) => {
                if attempt == 2 {
                    return Err(error);
                }
                std::thread::sleep(delay);
                delay *= 2;
            }
        }
    }
    unreachable!()
}

fn build_agent(proxy_url: Option<&str>) -> ureq::Agent {
    // Generous timeouts: a flaky upstream proxy or network jitter shouldn't make
    // a probe give up early and blank the account. Paired with store_quotas
    // keeping last-good, transient slowness no longer drops the numbers.
    let mut builder = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(10))
        .timeout_read(Duration::from_secs(30));
    // Route through the user's HTTP proxy (clash/v2ray etc.) like the original
    // macOS app's proxied URLSession (ProxyConfigurationService) — provider
    // endpoints are otherwise unreachable in many regions. Prefer the upstream
    // proxy URL configured in Settings; fall back to the standard proxy env vars.
    let chosen = proxy_url
        .map(str::trim)
        .filter(|url| !url.is_empty())
        .map(ToOwned::to_owned)
        .or_else(proxy_from_env);
    if let Some(url) = chosen {
        if let Ok(proxy) = ureq::Proxy::new(&url) {
            builder = builder.proxy(proxy);
        }
    }
    builder.build()
}

fn proxy_from_env() -> Option<String> {
    for key in [
        "HTTPS_PROXY",
        "https_proxy",
        "HTTP_PROXY",
        "http_proxy",
        "ALL_PROXY",
        "all_proxy",
    ] {
        if let Ok(value) = std::env::var(key) {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    None
}

enum FetchError {
    Unauthorized,
    Other,
}

/// List `~/.cli-proxy-api/<prefix>*.json` files (full paths + file names).
fn list_auth_files(prefix: &str) -> Vec<(PathBuf, String)> {
    let Some(dir) = auth_dir() else {
        return Vec::new();
    };
    let Ok(entries) = fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut files = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with(prefix) && name.ends_with(".json") {
            files.push((entry.path(), name));
        }
    }
    files
}

/// List Codex auth files by filename prefix OR JSON "type"/"provider" == codex,
/// so CPA-managed accounts (e.g. "*_cpa_*.json") are included too.
fn list_codex_auth_files() -> Vec<(PathBuf, String)> {
    let Some(dir) = auth_dir() else {
        return Vec::new();
    };
    let Ok(entries) = fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut files = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.ends_with(".json") || name.starts_with("glm-keys") {
            continue;
        }
        let is_codex = name.starts_with("codex-")
            || fs::read_to_string(entry.path())
                .ok()
                .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok())
                .and_then(|value| {
                    value
                        .get("type")
                        .or_else(|| value.get("provider"))
                        .and_then(|kind| kind.as_str())
                        .map(|kind| kind.eq_ignore_ascii_case("codex"))
                })
                .unwrap_or(false);
        if is_codex {
            files.push((entry.path(), name));
        }
    }
    files
}

fn clean_filename(filename: &str, prefix: &str) -> String {
    let trimmed = filename.strip_prefix(prefix).unwrap_or(filename);
    trimmed.strip_suffix(".json").unwrap_or(trimmed).to_string()
}

/// Relative "time until reset" label, e.g. "2d 17h" / "4h 28m" / "12m".
fn format_reset_secs(secs: i64) -> Option<String> {
    if secs <= 0 {
        return None;
    }
    let days = secs / 86_400;
    let hours = (secs % 86_400) / 3_600;
    let minutes = (secs % 3_600) / 60;
    Some(if days > 0 {
        format!("{}d {}h", days, hours)
    } else if hours > 0 {
        format!("{}h {}m", hours, minutes)
    } else {
        format!("{}m", minutes.max(1))
    })
}

fn now_unix() -> i64 {
    quotio_platform::current_unix_seconds() as i64
}

fn format_reset_unix(reset_at_unix: i64) -> Option<String> {
    if reset_at_unix <= 0 {
        return None;
    }
    format_reset_secs(reset_at_unix - now_unix())
}

fn format_reset_iso(iso: &str) -> Option<String> {
    if iso.trim().is_empty() {
        return None;
    }
    let dt = DateTime::parse_from_rfc3339(iso.trim()).ok()?;
    format_reset_secs(dt.timestamp() - now_unix())
}

fn model_usage(name: &str, remaining_percent: f64, reset_at: Option<String>) -> QuotaModelUsage {
    let remaining = remaining_percent.clamp(0.0, 100.0);
    QuotaModelUsage {
        model: name.to_string(),
        used_percent: (100.0 - remaining).clamp(0.0, 100.0),
        remaining_percent: remaining,
        reset_at,
        reset_at_unix: None,
    }
}

/// 同 [`model_usage`]，但从原始 unix 秒生成展示字符串，并保留原始值
/// （`reset_at_unix` 给智能调度规则用）。
fn model_usage_unix(name: &str, remaining_percent: f64, reset_at_unix: Option<i64>) -> QuotaModelUsage {
    let mut usage = model_usage(
        name,
        remaining_percent,
        reset_at_unix.and_then(format_reset_unix),
    );
    usage.reset_at_unix = reset_at_unix.filter(|unix| *unix > 0);
    usage
}

/// Fetch a provider's accounts concurrently (bounded), so one slow or
/// unreachable account never serializes the rest. The whole refresh is then
/// bounded by the slowest account, not the sum — which is what made an
/// unreachable proxy appear to hang for minutes with many accounts.
fn fetch_parallel<F>(
    agent: &ureq::Agent,
    files: Vec<(PathBuf, String)>,
    fetch_one: F,
    emit: &(dyn Fn(&AccountQuota) + Sync),
) -> Vec<AccountQuota>
where
    F: Fn(&ureq::Agent, &Path, &str) -> Option<AccountQuota> + Sync,
{
    const MAX_CONCURRENCY: usize = 10;
    let mut out = Vec::new();
    for chunk in files.chunks(MAX_CONCURRENCY) {
        let results: Vec<Option<AccountQuota>> = std::thread::scope(|scope| {
            let handles: Vec<_> = chunk
                .iter()
                .map(|(path, name)| {
                    scope.spawn(|| {
                        let quota = fetch_one(agent, path, name);
                        if let Some(account) = &quota {
                            emit(account); // stream each account to the UI the moment it lands
                        }
                        quota
                    })
                })
                .collect();
            handles
                .into_iter()
                .map(|handle| handle.join().unwrap_or(None))
                .collect()
        });
        out.extend(results.into_iter().flatten());
    }
    out
}

// ===================== Codex / OpenAI =====================

fn fetch_codex_quotas(
    agent: &ureq::Agent,
    emit: &(dyn Fn(&AccountQuota) + Sync),
) -> Vec<AccountQuota> {
    fetch_parallel(agent, list_codex_auth_files(), fetch_codex_one, emit)
}

// ===================== Gemini CLI =====================

fn fetch_gemini_quotas(
    agent: &ureq::Agent,
    emit: &(dyn Fn(&AccountQuota) + Sync),
) -> Vec<AccountQuota> {
    fetch_parallel(agent, list_auth_files("gemini-"), fetch_gemini_one, emit)
}

/// Direct (best-effort) Gemini CLI quota: reads access_token + project from the
/// auth file and queries retrieveUserQuota. Returns None (no card) on any
/// failure, so it never shows placeholder data. NOTE: unverified — built without
/// a Gemini account; refine once one is available to test against.
fn fetch_gemini_one(agent: &ureq::Agent, path: &Path, filename: &str) -> Option<AccountQuota> {
    let raw = fs::read_to_string(path).ok()?;
    let value: serde_json::Value = serde_json::from_str(&raw).ok()?;
    let access_token = value
        .get("access_token")
        .and_then(|token| token.as_str())
        .filter(|token| !token.is_empty())?
        .to_string();
    let project = value
        .get("project_id")
        .or_else(|| value.get("project"))
        .and_then(|project| project.as_str())
        .filter(|project| !project.is_empty())?
        .to_string();
    let label = value
        .get("email")
        .and_then(|email| email.as_str())
        .map(str::to_string)
        .or_else(|| {
            value
                .get("id_token")
                .and_then(|token| token.as_str())
                .and_then(decode_jwt_email)
        })
        .unwrap_or_else(|| clean_filename(filename, "gemini-"));

    let response = agent
        .post("https://cloudcode-pa.googleapis.com/v1internal:retrieveUserQuota")
        .set("Authorization", &format!("Bearer {}", access_token))
        .set("Content-Type", "application/json")
        .send_string(&format!("{{\"project\":\"{}\"}}", project))
        .ok()?;
    let payload: serde_json::Value = response.into_json().ok()?;
    let buckets = payload.get("buckets")?.as_array()?;

    let mut models = Vec::new();
    for bucket in buckets {
        let Some(model_id) = bucket
            .get("modelId")
            .or_else(|| bucket.get("model_id"))
            .and_then(|id| id.as_str())
        else {
            continue;
        };
        let remaining = bucket
            .get("remainingFraction")
            .or_else(|| bucket.get("remaining_fraction"))
            .and_then(|fraction| fraction.as_f64())
            .unwrap_or(0.0);
        let reset = bucket
            .get("resetTime")
            .or_else(|| bucket.get("reset_time"))
            .and_then(|reset| reset.as_str())
            .and_then(format_reset_iso);
        models.push(model_usage(model_id, remaining * 100.0, reset));
    }
    if models.is_empty() {
        return None;
    }
    Some(AccountQuota {
        provider_id: "gemini-cli".to_string(),
        account_label: label,
        account_key: clean_filename(filename, "gemini-"),
        is_forbidden: false,
        status_message: None,
        models,
    })
}

fn fetch_codex_one(agent: &ureq::Agent, path: &Path, filename: &str) -> Option<AccountQuota> {
    let raw = fs::read_to_string(path).ok()?;
    let auth: CodexAuthFile = serde_json::from_str(&raw).ok()?;
    let raw_json: serde_json::Value = serde_json::from_str(&raw).unwrap_or(serde_json::Value::Null);

    let account_id = resolve_codex_account_id(&auth, &raw_json);
    let key = clean_filename(filename, "codex-");
    let label = auth
        .email
        .clone()
        .filter(|email| !email.trim().is_empty())
        .or_else(|| auth.id_token.as_deref().and_then(decode_jwt_email))
        .unwrap_or_else(|| key.clone());

    // Proactively refresh an expired access token and write it back to the auth
    // file (mirrors the macOS app), so both the quota fetch and the proxy use a
    // fresh token instead of hitting 401.
    let mut access_token = auth.access_token.clone();
    if jwt_token_expired(&access_token) {
        if let Some(refresh_token) = auth.refresh_token.as_deref() {
            if let Ok(refreshed) = refresh_codex_token(agent, refresh_token) {
                write_codex_access_token(path, &refreshed);
                access_token = refreshed;
            }
        }
    }

    let mut auth_failed = false;
    let usage = match fetch_codex_usage(agent, &access_token, account_id.as_deref()) {
        Ok(usage) => Some(usage),
        Err(FetchError::Unauthorized) => {
            let recovered = auth
                .refresh_token
                .as_deref()
                .and_then(|token| refresh_codex_token(agent, token).ok())
                .and_then(|refreshed| {
                    write_codex_access_token(path, &refreshed);
                    fetch_codex_usage(agent, &refreshed, account_id.as_deref()).ok()
                });
            // A 401 we couldn't recover (no/expired refresh token) means the
            // account needs re-authorization — flag it for the Providers page.
            auth_failed = recovered.is_none();
            recovered
        }
        // Transient failure (proxy not ready yet on startup / slow / rate limit) —
        // wait briefly and retry once so a single hiccup doesn't drop the account.
        Err(FetchError::Other) => {
            std::thread::sleep(Duration::from_millis(400));
            match fetch_codex_usage(agent, &access_token, account_id.as_deref()) {
                Ok(usage) => Some(usage),
                // The retry surfaced a real 401 — refresh + retry and flag re-auth
                // if that fails, exactly like the primary Unauthorized arm. Without
                // this, a token that expired during a transient blip is shown as a
                // healthy account and the scheduler keeps picking a dead token.
                Err(FetchError::Unauthorized) => {
                    let recovered = auth
                        .refresh_token
                        .as_deref()
                        .and_then(|token| refresh_codex_token(agent, token).ok())
                        .and_then(|refreshed| {
                            write_codex_access_token(path, &refreshed);
                            fetch_codex_usage(agent, &refreshed, account_id.as_deref()).ok()
                        });
                    auth_failed = recovered.is_none();
                    recovered
                }
                Err(FetchError::Other) => None,
            }
        }
    };

    let Some(usage) = usage else {
        // Usage unavailable (token expired / network) — still list the account
        // (no quota bars) so it stays visible and its health sparkline shows,
        // instead of silently dropping the very accounts that are failing.
        return Some(AccountQuota {
            provider_id: "codex".to_string(),
            account_label: label,
            account_key: key,
            is_forbidden: false,
            status_message: if auth_failed {
                Some("auth_failed".to_string())
            } else {
                codex_plan_status(&auth, None, None)
            },
            models: Vec::new(),
        });
    };

    let rate = usage.rate_limit.unwrap_or_default();
    let session_used = rate
        .primary_window
        .as_ref()
        .and_then(|w| w.used_percent)
        .unwrap_or(0);
    let weekly_used = rate
        .secondary_window
        .as_ref()
        .and_then(|w| w.used_percent)
        .unwrap_or(0);
    let session_reset = rate.primary_window.as_ref().and_then(|w| w.reset_at);
    let weekly_reset = rate.secondary_window.as_ref().and_then(|w| w.reset_at);
    let reset_credits = usage
        .rate_limit_reset_credits
        .as_ref()
        .and_then(|credits| credits.available_count);

    // 当前能不能用——直接信 API 自己的闸门（`rate_limit.allowed` / `limit_reached`），
    // 它同时反映 5h（primary）和周（secondary）两个窗口：周额度打满的账号即便 5h 窗口
    // 几乎全空，API 也会返回 `allowed: false`，发任何请求都会 429，所以必须移出代理池、
    // 等窗口刷新再回来。仅当 API 没给这两个字段时，才退回用 `session_used >= 100` 兜底。
    let blocked = rate.allowed == Some(false)
        || rate.limit_reached == Some(true)
        || session_used >= 100;
    Some(AccountQuota {
        provider_id: "codex".to_string(),
        account_label: label,
        account_key: key,
        is_forbidden: blocked,
        status_message: codex_plan_status(&auth, usage.plan_type.as_deref(), reset_credits),
        models: vec![
            model_usage_unix(
                "Session",
                (100 - session_used.clamp(0, 100)) as f64,
                session_reset,
            ),
            model_usage_unix(
                "Weekly",
                (100 - weekly_used.clamp(0, 100)) as f64,
                weekly_reset,
            ),
        ],
    })
}

fn fetch_codex_usage(
    agent: &ureq::Agent,
    access_token: &str,
    account_id: Option<&str>,
) -> Result<CodexUsageResponse, FetchError> {
    let response = call_with_retry(|| {
        let mut request = agent
            .get(CODEX_USAGE_URL)
            .set("Authorization", &format!("Bearer {}", access_token))
            .set("Accept", "application/json");
        if let Some(id) = account_id {
            if !id.is_empty() {
                request = request.set("ChatGPT-Account-Id", id);
            }
        }
        request
    });

    match response {
        Ok(response) => response
            .into_json::<CodexUsageResponse>()
            .map_err(|_| FetchError::Other),
        Err(ureq::Error::Status(401, _)) | Err(ureq::Error::Status(403, _)) => {
            Err(FetchError::Unauthorized)
        }
        Err(_) => Err(FetchError::Other),
    }
}

fn refresh_codex_token(agent: &ureq::Agent, refresh_token: &str) -> Result<String, FetchError> {
    let body = format!("refresh_token={}", urlencoding::encode(refresh_token));
    match agent
        .post(CODEX_TOKEN_REFRESH_URL)
        .set("Content-Type", "application/x-www-form-urlencoded")
        .send_string(&body)
    {
        Ok(response) => response
            .into_json::<TokenRefreshResponse>()
            .map(|parsed| parsed.access_token)
            .map_err(|_| FetchError::Other),
        Err(_) => Err(FetchError::Other),
    }
}

/// Spend one "主动重置次数" (rate-limit reset credit) to force-reset the Codex
/// 5h primary window for the account whose key matches `account_key`. Mirrors the
/// CLIProxyAPI Management Center's reset action exactly: POST consume with a fresh
/// `redeem_request_id`, using the account's OAuth token (refreshed on 401). On
/// success the proactively-refreshed token is written back so the next usage fetch
/// reflects the reset. Returns a localized error message on failure (e.g. no
/// credits available, network, or re-auth needed).
pub fn consume_codex_reset_credit(account_key: &str, proxy_url: Option<&str>) -> Result<(), String> {
    let agent = build_agent(proxy_url);
    let (path, _name) = list_codex_auth_files()
        .into_iter()
        .find(|(_, name)| clean_filename(name, "codex-") == account_key)
        .ok_or_else(|| format!("未找到账号文件：{}", account_key))?;
    let raw = fs::read_to_string(&path).map_err(|err| format!("读取账号文件失败：{}", err))?;
    let auth: CodexAuthFile =
        serde_json::from_str(&raw).map_err(|err| format!("解析账号文件失败：{}", err))?;
    let raw_json: serde_json::Value = serde_json::from_str(&raw).unwrap_or(serde_json::Value::Null);
    let account_id = resolve_codex_account_id(&auth, &raw_json);
    // One redeem id reused across the (rare) refresh-and-retry, so a token that
    // turned out to be valid can't double-spend a credit.
    let redeem_id = uuid::Uuid::new_v4().to_string();

    let mut access_token = auth.access_token.clone();
    if jwt_token_expired(&access_token) {
        if let Some(refresh_token) = auth.refresh_token.as_deref() {
            if let Ok(refreshed) = refresh_codex_token(&agent, refresh_token) {
                write_codex_access_token(&path, &refreshed);
                access_token = refreshed;
            }
        }
    }

    match post_codex_reset(&agent, &access_token, account_id.as_deref(), &redeem_id) {
        Ok(()) => Ok(()),
        Err(ResetError::Unauthorized) => {
            let refresh_token = auth
                .refresh_token
                .as_deref()
                .ok_or_else(|| "账号需要重新授权".to_string())?;
            let refreshed = refresh_codex_token(&agent, refresh_token)
                .map_err(|_| "令牌刷新失败，账号可能需要重新授权".to_string())?;
            write_codex_access_token(&path, &refreshed);
            post_codex_reset(&agent, &refreshed, account_id.as_deref(), &redeem_id).map_err(
                |err| match err {
                    ResetError::Unauthorized => "重置失败：令牌刷新后仍被拒绝".to_string(),
                    ResetError::Failed(message) => message,
                },
            )
        }
        Err(ResetError::Failed(message)) => Err(message),
    }
}

enum ResetError {
    Unauthorized,
    Failed(String),
}

fn post_codex_reset(
    agent: &ureq::Agent,
    access_token: &str,
    account_id: Option<&str>,
    redeem_id: &str,
) -> Result<(), ResetError> {
    let mut request = agent
        .post(CODEX_RESET_CREDITS_URL)
        .set("Authorization", &format!("Bearer {}", access_token))
        .set("Content-Type", "application/json")
        .set("User-Agent", CODEX_USER_AGENT);
    if let Some(id) = account_id {
        if !id.is_empty() {
            request = request.set("Chatgpt-Account-Id", id);
        }
    }
    match request.send_json(serde_json::json!({ "redeem_request_id": redeem_id })) {
        Ok(_) => Ok(()),
        Err(ureq::Error::Status(401, _)) | Err(ureq::Error::Status(403, _)) => {
            Err(ResetError::Unauthorized)
        }
        Err(ureq::Error::Status(code, response)) => {
            // Surface the server's reason (e.g. no credits left) so the card can
            // show why the reset didn't happen, not just a generic failure.
            let body = response.into_string().unwrap_or_default();
            // char-based truncation so a multibyte boundary can't panic.
            let snippet: String = body.trim().chars().take(200).collect();
            Err(ResetError::Failed(if snippet.is_empty() {
                format!("重置失败（HTTP {}）", code)
            } else {
                format!("重置失败（HTTP {}）：{}", code, snippet)
            }))
        }
        Err(_) => Err(ResetError::Failed("重置失败：网络异常".to_string())),
    }
}

/// Whether a JWT access token is expired (or within a 60s buffer of expiry).
/// Returns false when the `exp` claim can't be read, so we don't refresh blindly.
fn jwt_token_expired(token: &str) -> bool {
    match decode_jwt_payload(token)
        .and_then(|claims| claims.get("exp").and_then(|exp| exp.as_f64()))
    {
        Some(exp) => exp < (now_unix() as f64) + 60.0,
        None => false,
    }
}

/// Atomically write a refreshed access token back into a Codex auth file,
/// preserving every other field, so both the quota fetch and the proxy pick up
/// the fresh token (mirrors the macOS app's updateAuthFile). Best-effort.
fn write_codex_access_token(path: &Path, new_token: &str) {
    let Ok(raw) = fs::read_to_string(path) else {
        return;
    };
    let Ok(mut value) = serde_json::from_str::<serde_json::Value>(&raw) else {
        return;
    };
    if let Some(object) = value.as_object_mut() {
        object.insert(
            "access_token".to_string(),
            serde_json::Value::String(new_token.to_string()),
        );
    }
    if let Ok(serialized) = serde_json::to_vec_pretty(&value) {
        let _ = quotio_platform::atomic_write(path, &serialized, true);
    }
}

fn resolve_codex_account_id(auth: &CodexAuthFile, raw: &serde_json::Value) -> Option<String> {
    if let Some(id) = auth.account_id.as_deref() {
        if !id.is_empty() {
            return Some(id.to_string());
        }
    }
    if let Some(id) = raw
        .get("chatgpt_account_id")
        .and_then(|value| value.as_str())
    {
        if !id.is_empty() {
            return Some(id.to_string());
        }
    }
    auth.id_token.as_deref().and_then(decode_jwt_account_id)
}

pub(crate) fn decode_jwt_payload(token: &str) -> Option<serde_json::Value> {
    let segment = token.split('.').nth(1)?.trim_end_matches('=');
    let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(segment)
        .ok()?;
    serde_json::from_slice(&decoded).ok()
}

fn decode_jwt_account_id(token: &str) -> Option<String> {
    let payload = decode_jwt_payload(token)?;
    payload
        .get("https://api.openai.com/auth")
        .and_then(|value| value.get("chatgpt_account_id"))
        .and_then(|value| value.as_str())
        .filter(|id| !id.is_empty())
        .map(ToString::to_string)
}

fn decode_jwt_email(token: &str) -> Option<String> {
    let payload = decode_jwt_payload(token)?;
    payload
        .get("email")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|email| !email.is_empty())
        .map(ToString::to_string)
}

/// Pull the subscription tier + expiry date out of a ChatGPT id_token JWT.
/// The usage API does not return these; the id_token's auth claim does.
fn decode_jwt_plan(token: &str) -> Option<(Option<String>, Option<String>)> {
    let payload = decode_jwt_payload(token)?;
    let auth = payload.get("https://api.openai.com/auth")?;
    let plan = auth
        .get("chatgpt_plan_type")
        .and_then(|value| value.as_str())
        .filter(|plan| !plan.is_empty())
        .map(ToString::to_string);
    let until = auth
        .get("chatgpt_subscription_active_until")
        .and_then(|value| value.as_str())
        .filter(|value| !value.is_empty())
        .map(ToString::to_string);
    Some((plan, until))
}

/// Encode plan tier + expiry + reset credits into the status_message as
/// "plan: <tier> | until: <YYYY-MM-DD> | resets: <N>" for the Quota card to
/// parse. Any piece may be absent; returns None only when nothing is known.
fn codex_plan_status(
    auth: &CodexAuthFile,
    usage_plan: Option<&str>,
    reset_credits: Option<i64>,
) -> Option<String> {
    let (jwt_plan, until) = auth
        .id_token
        .as_deref()
        .and_then(decode_jwt_plan)
        .unwrap_or((None, None));
    let plan = jwt_plan.or_else(|| usage_plan.map(ToString::to_string));
    let mut parts: Vec<String> = Vec::new();
    if let Some(plan) = plan {
        parts.push(format!("plan: {}", plan));
    }
    if let Some(date) = until.as_deref().and_then(|value| value.split('T').next()) {
        parts.push(format!("until: {}", date));
    }
    if let Some(credits) = reset_credits {
        parts.push(format!("resets: {}", credits));
    }
    if parts.is_empty() {
        return None;
    }
    Some(parts.join(" | "))
}

fn cursor_state_db_path() -> Option<std::path::PathBuf> {
    let appdata = std::env::var_os("APPDATA")?;
    Some(
        std::path::PathBuf::from(appdata)
            .join("Cursor")
            .join("User")
            .join("globalStorage")
            .join("state.vscdb"),
    )
}

/// Read Cursor's stored auth (access token + email + membership) from its
/// state.vscdb SQLite DB (read-only + immutable so a missing WAL file or a
/// running Cursor instance doesn't block the read).
fn read_cursor_auth() -> Option<(String, Option<String>, Option<String>)> {
    let path = cursor_state_db_path()?;
    if !path.exists() {
        return None;
    }
    let uri = format!(
        "file:///{}?mode=ro&immutable=1",
        path.to_string_lossy().replace('\\', "/")
    );
    let conn = rusqlite::Connection::open_with_flags(
        uri,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_URI,
    )
    .ok()?;
    let rows: Vec<(String, String)> = {
        let mut stmt = conn
            .prepare("SELECT key, value FROM ItemTable WHERE key LIKE 'cursorAuth/%'")
            .ok()?;
        let mapped = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .ok()?;
        mapped.flatten().collect()
    };
    let mut token = None;
    let mut email = None;
    let mut membership = None;
    for (key, value) in rows {
        match key.as_str() {
            "cursorAuth/accessToken" => token = Some(value),
            "cursorAuth/cachedEmail" => email = Some(value),
            "cursorAuth/stripeMembershipType" => membership = Some(value),
            _ => {}
        }
    }
    token
        .filter(|value| !value.is_empty())
        .map(|value| (value, email, membership))
}

fn fetch_cursor_quotas(agent: &ureq::Agent) -> Vec<AccountQuota> {
    let Some((token, email, membership)) = read_cursor_auth() else {
        return Vec::new();
    };
    let label = email
        .clone()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "Cursor".to_string());
    let summary = agent
        .get("https://api2.cursor.sh/auth/usage-summary")
        .set("Authorization", &format!("Bearer {}", token))
        .set("Accept", "application/json")
        .set("User-Agent", "Mozilla/5.0")
        .call()
        .ok()
        .and_then(|resp| resp.into_json::<CursorUsageSummary>().ok());

    let mut models = Vec::new();
    let mut plan_name = membership;
    let mut until: Option<String> = None;
    if let Some(summary) = summary {
        if summary.membership_type.is_some() {
            plan_name = summary.membership_type;
        }
        until = summary.billing_cycle_end.clone();
        if let Some(usage) = summary.individual_usage {
            if let Some(plan) = usage.plan {
                if plan.enabled.unwrap_or(false) {
                    let limit = plan.limit.unwrap_or(0);
                    let remaining = plan.remaining.unwrap_or(0);
                    let percent = if limit > 0 {
                        (remaining as f64 / limit as f64 * 100.0).clamp(0.0, 100.0)
                    } else {
                        100.0
                    };
                    let reset = until.as_deref().and_then(format_reset_iso);
                    models.push(model_usage("Plan", percent, reset));
                }
            }
            if let Some(on_demand) = usage.on_demand {
                if on_demand.enabled.unwrap_or(false) {
                    let percent = match (on_demand.limit, on_demand.remaining) {
                        (Some(limit), Some(remaining)) if limit > 0 => {
                            (remaining as f64 / limit as f64 * 100.0).clamp(0.0, 100.0)
                        }
                        _ => 100.0,
                    };
                    models.push(model_usage("On-Demand", percent, None));
                }
            }
        }
    }
    if models.is_empty() {
        models.push(model_usage("Cursor", 100.0, None));
    }
    let status_message = plan_name.filter(|value| !value.is_empty()).map(|plan| {
        let mut status = format!("plan: {}", plan);
        if let Some(date) = until.as_deref().and_then(|value| value.split('T').next()) {
            status.push_str(" | until: ");
            status.push_str(date);
        }
        status
    });

    vec![AccountQuota {
        provider_id: "cursor".to_string(),
        account_label: label,
        account_key: "cursor".to_string(),
        is_forbidden: false,
        status_message,
        models,
    }]
}

#[derive(Debug, Deserialize)]
struct CursorUsageSummary {
    #[serde(rename = "membershipType")]
    membership_type: Option<String>,
    #[serde(rename = "billingCycleEnd")]
    billing_cycle_end: Option<String>,
    #[serde(rename = "individualUsage")]
    individual_usage: Option<CursorIndividualUsage>,
}

#[derive(Debug, Deserialize)]
struct CursorIndividualUsage {
    plan: Option<CursorPlanUsage>,
    #[serde(rename = "onDemand")]
    on_demand: Option<CursorOnDemand>,
}

#[derive(Debug, Deserialize)]
struct CursorPlanUsage {
    enabled: Option<bool>,
    limit: Option<i64>,
    remaining: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct CursorOnDemand {
    enabled: Option<bool>,
    limit: Option<i64>,
    remaining: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct CodexAuthFile {
    access_token: String,
    #[serde(default)]
    account_id: Option<String>,
    #[serde(default)]
    email: Option<String>,
    #[serde(default)]
    id_token: Option<String>,
    #[serde(default)]
    refresh_token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CodexUsageResponse {
    #[serde(default)]
    plan_type: Option<String>,
    #[serde(default)]
    rate_limit: Option<RateLimitInfo>,
    /// 主动重置次数 — manual rate-limit reset credits (`{ available_count }`).
    #[serde(default)]
    rate_limit_reset_credits: Option<ResetCreditsInfo>,
}

#[derive(Debug, Default, Deserialize)]
struct ResetCreditsInfo {
    #[serde(default)]
    available_count: Option<i64>,
}

#[derive(Debug, Default, Deserialize)]
struct RateLimitInfo {
    /// API 的总闸门：false = 当前不允许请求（任一窗口打满都会置 false）。
    #[serde(default)]
    allowed: Option<bool>,
    #[serde(default)]
    limit_reached: Option<bool>,
    #[serde(default)]
    primary_window: Option<WindowInfo>,
    #[serde(default)]
    secondary_window: Option<WindowInfo>,
}

#[derive(Debug, Deserialize)]
struct WindowInfo {
    #[serde(default)]
    used_percent: Option<i64>,
    #[serde(default)]
    reset_at: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct TokenRefreshResponse {
    access_token: String,
}

// ===================== Claude Code =====================

fn fetch_claude_quotas(agent: &ureq::Agent) -> Vec<AccountQuota> {
    let mut quotas = Vec::new();
    for (path, name) in list_auth_files("claude-") {
        if let Some(quota) = fetch_claude_one(agent, &path, &name) {
            quotas.push(quota);
        }
    }
    quotas
}

fn fetch_claude_one(agent: &ureq::Agent, path: &Path, filename: &str) -> Option<AccountQuota> {
    let raw = fs::read_to_string(path).ok()?;
    let auth: ClaudeAuthFile = serde_json::from_str(&raw).ok()?;
    let key = clean_filename(filename, "claude-");
    let label = auth
        .email
        .clone()
        .filter(|email| !email.trim().is_empty())
        .unwrap_or_else(|| key.clone());

    let usage = match fetch_claude_usage(agent, &auth.access_token) {
        Ok(usage) => Some(usage),
        Err(FetchError::Unauthorized) => {
            let refresh_token = auth.refresh_token.as_deref();
            match refresh_token.and_then(|token| refresh_claude_token(agent, token).ok()) {
                Some(new_token) => fetch_claude_usage(agent, &new_token).ok(),
                None => {
                    // Refresh impossible/failed → mark account as needs re-auth.
                    return Some(AccountQuota {
                        provider_id: "claude".to_string(),
                        account_label: label,
                        account_key: key,
                        is_forbidden: true,
                        status_message: Some("需要重新授权".to_string()),
                        models: Vec::new(),
                    });
                }
            }
        }
        Err(FetchError::Other) => None,
    }?;

    let mut models = Vec::new();
    if let Some(window) = usage.five_hour {
        models.push(claude_window_model("Session (5h)", window));
    }
    if let Some(window) = usage.seven_day {
        models.push(claude_window_model("Weekly", window));
    }
    if let Some(window) = usage.seven_day_sonnet {
        models.push(claude_window_model("Sonnet (7d)", window));
    }
    if let Some(window) = usage.seven_day_opus {
        models.push(claude_window_model("Opus (7d)", window));
    }
    if let Some(extra) = usage.extra_usage {
        if extra.is_enabled.unwrap_or(false) {
            if let Some(util) = extra.utilization {
                models.push(model_usage("Extra usage", 100.0 - util, None));
            }
        }
    }

    // 探测成功但没有任何用量窗口 = 账号健康、只是没有额度条数据。返回 Some(空 models)
    // 而非 None,这样被健康隔离的号在鉴权恢复后即便没拉到窗口,也能被对账确证健康、解除隔离。
    Some(AccountQuota {
        provider_id: "claude".to_string(),
        account_label: label,
        account_key: key,
        is_forbidden: false,
        status_message: None,
        models,
    })
}

fn claude_window_model(name: &str, window: ClaudeWindow) -> QuotaModelUsage {
    let util = window.utilization.unwrap_or(0.0);
    let reset_unix = window
        .resets_at
        .as_deref()
        .and_then(|iso| DateTime::parse_from_rfc3339(iso.trim()).ok())
        .map(|dt| dt.timestamp());
    model_usage_unix(name, 100.0 - util, reset_unix)
}

fn fetch_claude_usage(
    agent: &ureq::Agent,
    access_token: &str,
) -> Result<ClaudeUsageResponse, FetchError> {
    let response = agent
        .get(CLAUDE_USAGE_URL)
        .set("Accept", "application/json")
        .set("Authorization", &format!("Bearer {}", access_token))
        .set("anthropic-beta", "oauth-2025-04-20")
        .call();

    match response {
        Ok(resp) => {
            let parsed: ClaudeUsageResponse = resp.into_json().map_err(|_| FetchError::Other)?;
            if parsed.kind.as_deref() == Some("error") {
                return Err(FetchError::Unauthorized);
            }
            Ok(parsed)
        }
        Err(ureq::Error::Status(401, _)) | Err(ureq::Error::Status(403, _)) => {
            Err(FetchError::Unauthorized)
        }
        Err(_) => Err(FetchError::Other),
    }
}

fn refresh_claude_token(agent: &ureq::Agent, refresh_token: &str) -> Result<String, FetchError> {
    let body = format!(
        "grant_type=refresh_token&refresh_token={}&client_id={}",
        urlencoding::encode(refresh_token),
        CLAUDE_CLIENT_ID
    );
    match agent
        .post(CLAUDE_TOKEN_URL)
        .set("Content-Type", "application/x-www-form-urlencoded")
        .send_string(&body)
    {
        Ok(response) => response
            .into_json::<TokenRefreshResponse>()
            .map(|parsed| parsed.access_token)
            .map_err(|_| FetchError::Other),
        Err(_) => Err(FetchError::Other),
    }
}

#[derive(Debug, Deserialize)]
struct ClaudeAuthFile {
    access_token: String,
    #[serde(default)]
    email: Option<String>,
    #[serde(default)]
    refresh_token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ClaudeUsageResponse {
    #[serde(rename = "type", default)]
    kind: Option<String>,
    #[serde(default)]
    five_hour: Option<ClaudeWindow>,
    #[serde(default)]
    seven_day: Option<ClaudeWindow>,
    #[serde(default)]
    seven_day_sonnet: Option<ClaudeWindow>,
    #[serde(default)]
    seven_day_opus: Option<ClaudeWindow>,
    #[serde(default)]
    extra_usage: Option<ClaudeExtra>,
}

#[derive(Debug, Deserialize)]
struct ClaudeWindow {
    #[serde(default)]
    utilization: Option<f64>,
    #[serde(default)]
    resets_at: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ClaudeExtra {
    #[serde(default)]
    is_enabled: Option<bool>,
    #[serde(default)]
    utilization: Option<f64>,
}

// ===================== GitHub Copilot =====================

fn fetch_copilot_quotas(agent: &ureq::Agent) -> Vec<AccountQuota> {
    let mut quotas = Vec::new();
    for (path, name) in list_auth_files("github-copilot-") {
        if let Some(quota) = fetch_copilot_one(agent, &path, &name) {
            quotas.push(quota);
        }
    }
    quotas
}

fn fetch_copilot_one(agent: &ureq::Agent, path: &Path, filename: &str) -> Option<AccountQuota> {
    let raw = fs::read_to_string(path).ok()?;
    let auth: CopilotAuthFile = serde_json::from_str(&raw).ok()?;
    let key = clean_filename(filename, "github-copilot-");
    let label = auth
        .username
        .clone()
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| key.clone());

    let entitlement = match fetch_copilot_entitlement(agent, &auth.access_token) {
        Ok(entitlement) => entitlement,
        Err(FetchError::Unauthorized) => {
            return Some(AccountQuota {
                provider_id: "copilot".to_string(),
                account_label: label,
                account_key: key,
                is_forbidden: true,
                status_message: Some("需要重新授权".to_string()),
                models: Vec::new(),
            });
        }
        Err(FetchError::Other) => return None,
    };

    let reset = entitlement
        .quota_reset_date_utc
        .as_deref()
        .or(entitlement.quota_reset_date.as_deref())
        .or(entitlement.limited_user_reset_date.as_deref())
        .and_then(format_reset_iso);

    let mut models = Vec::new();
    if let Some(snapshots) = entitlement.quota_snapshots.as_ref() {
        if let Some(chat) = snapshots.chat.as_ref() {
            if chat.unlimited != Some(true) {
                models.push(model_usage(
                    "Chat",
                    copilot_percent(chat, 50),
                    reset.clone(),
                ));
            }
        }
        if let Some(completions) = snapshots.completions.as_ref() {
            if completions.unlimited != Some(true) {
                models.push(model_usage(
                    "Completions",
                    copilot_percent(completions, 2000),
                    reset.clone(),
                ));
            }
        }
        if let Some(premium) = snapshots.premium_interactions.as_ref() {
            if premium.unlimited != Some(true) {
                models.push(model_usage(
                    "Premium",
                    copilot_percent(premium, 50),
                    reset.clone(),
                ));
            }
        }
    }
    if models.is_empty() {
        if let (Some(remaining), Some(total)) = (
            entitlement.limited_user_quotas.as_ref(),
            entitlement.monthly_quotas.as_ref(),
        ) {
            if let (Some(chat_remaining), Some(chat_total)) = (remaining.chat, total.chat) {
                if chat_total > 0 {
                    models.push(model_usage(
                        "Chat",
                        chat_remaining as f64 / chat_total as f64 * 100.0,
                        reset.clone(),
                    ));
                }
            }
            if let (Some(comp_remaining), Some(comp_total)) =
                (remaining.completions, total.completions)
            {
                if comp_total > 0 {
                    models.push(model_usage(
                        "Completions",
                        comp_remaining as f64 / comp_total as f64 * 100.0,
                        reset.clone(),
                    ));
                }
            }
        }
    }
    // 探测成功但无额度条 = 健康、只是没有用量数据;返回 Some(空) 而非 None,
    // 让健康隔离的号在恢复后即便没拉到额度也能被对账确证健康、解除隔离。
    Some(AccountQuota {
        provider_id: "copilot".to_string(),
        account_label: label,
        account_key: key,
        is_forbidden: false,
        status_message: Some(format!("plan: {}", copilot_plan_name(&entitlement))),
        models,
    })
}

fn fetch_copilot_entitlement(
    agent: &ureq::Agent,
    token: &str,
) -> Result<CopilotEntitlement, FetchError> {
    match agent
        .get(COPILOT_ENTITLEMENT_URL)
        .set("Authorization", &format!("Bearer {}", token))
        .set("Accept", "application/vnd.github+json")
        .set("X-GitHub-Api-Version", "2022-11-28")
        .call()
    {
        Ok(response) => response
            .into_json::<CopilotEntitlement>()
            .map_err(|_| FetchError::Other),
        Err(ureq::Error::Status(401, _)) | Err(ureq::Error::Status(403, _)) => {
            Err(FetchError::Unauthorized)
        }
        Err(_) => Err(FetchError::Other),
    }
}

/// Remaining-percent for a Copilot quota snapshot (mirrors `calculatePercent`).
fn copilot_percent(snapshot: &CopilotSnapshot, default_total: i64) -> f64 {
    if let Some(percent) = snapshot.percent_remaining {
        return percent.clamp(0.0, 100.0);
    }
    let remaining = snapshot.remaining.unwrap_or(0);
    let total = snapshot.entitlement.unwrap_or(default_total);
    if total > 0 {
        ((remaining as f64 / total as f64) * 100.0).clamp(0.0, 100.0)
    } else {
        0.0
    }
}

fn copilot_plan_name(entitlement: &CopilotEntitlement) -> String {
    let sku = entitlement
        .access_type_sku
        .clone()
        .unwrap_or_default()
        .to_lowercase();
    let plan = entitlement
        .copilot_plan
        .clone()
        .unwrap_or_default()
        .to_lowercase();
    if sku.contains("enterprise") || plan == "enterprise" {
        return "Enterprise".to_string();
    }
    if sku.contains("business") || plan == "business" {
        return "Business".to_string();
    }
    if sku.contains("educational") || sku.contains("pro") || plan.contains("pro") {
        return "Pro".to_string();
    }
    if plan == "individual" && !sku.contains("free_limited") {
        return "Pro".to_string();
    }
    if sku.contains("free_limited") || sku == "free" || plan.contains("free") {
        return "Free".to_string();
    }
    entitlement
        .copilot_plan
        .clone()
        .or_else(|| entitlement.access_type_sku.clone())
        .unwrap_or_else(|| "Copilot".to_string())
}

#[derive(Debug, Deserialize)]
struct CopilotAuthFile {
    access_token: String,
    #[serde(default)]
    username: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CopilotEntitlement {
    #[serde(default)]
    access_type_sku: Option<String>,
    #[serde(default)]
    copilot_plan: Option<String>,
    #[serde(default)]
    quota_reset_date: Option<String>,
    #[serde(default)]
    quota_reset_date_utc: Option<String>,
    #[serde(default)]
    limited_user_reset_date: Option<String>,
    #[serde(default)]
    quota_snapshots: Option<CopilotSnapshots>,
    #[serde(default)]
    limited_user_quotas: Option<CopilotLimitedQuotas>,
    #[serde(default)]
    monthly_quotas: Option<CopilotLimitedQuotas>,
}

#[derive(Debug, Deserialize)]
struct CopilotSnapshots {
    #[serde(default)]
    chat: Option<CopilotSnapshot>,
    #[serde(default)]
    completions: Option<CopilotSnapshot>,
    #[serde(default)]
    premium_interactions: Option<CopilotSnapshot>,
}

#[derive(Debug, Deserialize)]
struct CopilotSnapshot {
    #[serde(default)]
    entitlement: Option<i64>,
    #[serde(default)]
    remaining: Option<i64>,
    #[serde(default)]
    percent_remaining: Option<f64>,
    #[serde(default)]
    unlimited: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct CopilotLimitedQuotas {
    #[serde(default)]
    chat: Option<i64>,
    #[serde(default)]
    completions: Option<i64>,
}

// ===================== Antigravity (Google) =====================

fn fetch_antigravity_quotas(agent: &ureq::Agent) -> Vec<AccountQuota> {
    let mut quotas = Vec::new();
    for (path, name) in list_auth_files("antigravity-") {
        if let Some(quota) = fetch_antigravity_one(agent, &path, &name) {
            quotas.push(quota);
        }
    }
    quotas
}

fn fetch_antigravity_one(agent: &ureq::Agent, path: &Path, filename: &str) -> Option<AccountQuota> {
    let raw = fs::read_to_string(path).ok()?;
    let auth: AntigravityAuthFile = serde_json::from_str(&raw).ok()?;
    let key = clean_filename(filename, "antigravity-");
    let label = auth
        .email
        .clone()
        .filter(|email| !email.trim().is_empty())
        .unwrap_or_else(|| key.clone());

    let models = match fetch_antigravity_usage(agent, &auth.access_token) {
        Ok(models) => Some(models),
        Err(FetchError::Unauthorized) => {
            let refresh_token = auth.refresh_token.as_deref()?;
            let new_token = refresh_antigravity_token(agent, refresh_token).ok()?;
            fetch_antigravity_usage(agent, &new_token).ok()
        }
        Err(FetchError::Other) => None,
    }?;

    if models.is_empty() {
        return None;
    }

    Some(AccountQuota {
        provider_id: "antigravity".to_string(),
        account_label: label,
        account_key: key,
        is_forbidden: false,
        status_message: None,
        models,
    })
}

/// Fetch the project id (loadCodeAssist), then per-model quotas
/// (fetchAvailableModels). An auth error from either step bubbles up so the
/// caller can refresh the token and retry both.
fn fetch_antigravity_usage(
    agent: &ureq::Agent,
    token: &str,
) -> Result<Vec<QuotaModelUsage>, FetchError> {
    let project = match fetch_antigravity_project(agent, token) {
        Ok(project) => project,
        Err(FetchError::Unauthorized) => return Err(FetchError::Unauthorized),
        Err(FetchError::Other) => None,
    };
    fetch_antigravity_models(agent, token, project.as_deref())
}

fn fetch_antigravity_project(
    agent: &ureq::Agent,
    token: &str,
) -> Result<Option<String>, FetchError> {
    match agent
        .post(ANTIGRAVITY_PROJECT_URL)
        .set("Authorization", &format!("Bearer {}", token))
        .set("User-Agent", ANTIGRAVITY_USER_AGENT)
        .send_json(serde_json::json!({ "metadata": { "ideType": "ANTIGRAVITY" } }))
    {
        Ok(response) => {
            let info: AntigravitySubscription =
                response.into_json().map_err(|_| FetchError::Other)?;
            Ok(info.cloudaicompanion_project)
        }
        Err(ureq::Error::Status(401, _)) | Err(ureq::Error::Status(403, _)) => {
            Err(FetchError::Unauthorized)
        }
        Err(_) => Err(FetchError::Other),
    }
}

fn fetch_antigravity_models(
    agent: &ureq::Agent,
    token: &str,
    project: Option<&str>,
) -> Result<Vec<QuotaModelUsage>, FetchError> {
    let payload = match project {
        Some(project) if !project.is_empty() => serde_json::json!({ "project": project }),
        _ => serde_json::json!({}),
    };
    let response = match agent
        .post(ANTIGRAVITY_MODELS_URL)
        .set("Authorization", &format!("Bearer {}", token))
        .set("User-Agent", ANTIGRAVITY_USER_AGENT)
        .send_json(payload)
    {
        Ok(response) => response,
        Err(ureq::Error::Status(401, _)) => return Err(FetchError::Unauthorized),
        Err(_) => return Err(FetchError::Other),
    };

    let parsed: AntigravityQuotaResponse = response.into_json().map_err(|_| FetchError::Other)?;
    let mut models = Vec::new();
    for (name, info) in parsed.models {
        if !(name.contains("gemini") || name.contains("claude")) {
            continue;
        }
        if let Some(quota) = info.quota_info {
            let remaining = (quota.remaining_fraction.unwrap_or(0.0) * 100.0).clamp(0.0, 100.0);
            let reset = quota.reset_time.as_deref().and_then(format_reset_iso);
            models.push(model_usage(&name, remaining, reset));
        }
    }
    // HashMap iteration is unordered — sort for a stable display.
    models.sort_by(|a, b| a.model.cmp(&b.model));
    Ok(models)
}

fn refresh_antigravity_token(
    agent: &ureq::Agent,
    refresh_token: &str,
) -> Result<String, FetchError> {
    let body = format!(
        "client_id={}&client_secret={}&refresh_token={}&grant_type=refresh_token",
        ANTIGRAVITY_CLIENT_ID,
        ANTIGRAVITY_CLIENT_SECRET,
        urlencoding::encode(refresh_token)
    );
    match agent
        .post(ANTIGRAVITY_TOKEN_URL)
        .set("Content-Type", "application/x-www-form-urlencoded")
        .send_string(&body)
    {
        Ok(response) => response
            .into_json::<TokenRefreshResponse>()
            .map(|parsed| parsed.access_token)
            .map_err(|_| FetchError::Other),
        Err(_) => Err(FetchError::Other),
    }
}

#[derive(Debug, Deserialize)]
struct AntigravityAuthFile {
    access_token: String,
    #[serde(default)]
    email: Option<String>,
    #[serde(default)]
    refresh_token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AntigravitySubscription {
    #[serde(rename = "cloudaicompanionProject", default)]
    cloudaicompanion_project: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AntigravityQuotaResponse {
    #[serde(default)]
    models: std::collections::HashMap<String, AntigravityModelInfo>,
}

#[derive(Debug, Deserialize)]
struct AntigravityModelInfo {
    #[serde(rename = "quotaInfo", default)]
    quota_info: Option<AntigravityQuotaInfo>,
}

#[derive(Debug, Deserialize)]
struct AntigravityQuotaInfo {
    #[serde(rename = "remainingFraction", default)]
    remaining_fraction: Option<f64>,
    #[serde(rename = "resetTime", default)]
    reset_time: Option<String>,
}

// ===================== Kiro (AWS CodeWhisperer) =====================

fn fetch_kiro_quotas(agent: &ureq::Agent) -> Vec<AccountQuota> {
    let mut quotas = Vec::new();
    for (path, name) in list_auth_files("kiro-") {
        if let Some(quota) = fetch_kiro_one(agent, &path, &name) {
            quotas.push(quota);
        }
    }
    quotas
}

/// Refresh a Kiro access token via the Kiro auth refresh endpoint.
/// On success, updates the auth file on disk and returns the new access_token.
fn try_refresh_kiro_token(agent: &ureq::Agent, path: &Path, refresh_token: &str) -> Option<String> {
    let body = serde_json::json!({ "refreshToken": refresh_token });
    let resp = agent
        .post(KIRO_REFRESH_ENDPOINT)
        .set("Content-Type", "application/json")
        .send_string(&body.to_string())
        .ok()?;
    let resp_val: serde_json::Value = resp.into_json().ok()?;

    // The response may be wrapped in {"data": {...}}
    let token_data = resp_val
        .get("data")
        .filter(|v| v.is_object())
        .unwrap_or(&resp_val);

    let new_access = token_data
        .get("accessToken")
        .or_else(|| token_data.get("access_token"))
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())?;

    // Write refreshed tokens back to the auth file (preserve other fields).
    if let Ok(raw) = fs::read_to_string(path) {
        if let Ok(mut file_json) = serde_json::from_str::<serde_json::Value>(&raw) {
            if let Some(obj) = file_json.as_object_mut() {
                let at_key = if obj.contains_key("accessToken") {
                    "accessToken"
                } else {
                    "access_token"
                };
                obj.insert(at_key.to_string(), serde_json::Value::String(new_access.to_string()));

                if let Some(new_rt) = token_data
                    .get("refreshToken")
                    .or_else(|| token_data.get("refresh_token"))
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
                {
                    let rt_key = if obj.contains_key("refreshToken") {
                        "refreshToken"
                    } else {
                        "refresh_token"
                    };
                    obj.insert(rt_key.to_string(), serde_json::Value::String(new_rt.to_string()));
                }

                let _ = fs::write(path, serde_json::to_string_pretty(&file_json).unwrap_or_default());
            }
        }
    }

    Some(new_access.to_string())
}

fn call_kiro_usage(
    agent: &ureq::Agent,
    url: &str,
    access_token: &str,
    user_agent: &str,
) -> Result<KiroUsageResponse, ureq::Error> {
    agent
        .get(url)
        .set("Authorization", &format!("Bearer {}", access_token))
        .set("User-Agent", user_agent)
        .set("Accept", "application/json")
        .call()
        .and_then(|r| r.into_json().map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e).into()))
}

/// profileArn may sit at the top level or nested inside the cockpit-tools-style
/// `kiro_auth_token_raw` / `kiro_profile_raw` objects.
fn kiro_nested_profile_arn(auth: &KiroAuthFile) -> Option<String> {
    auth.kiro_auth_token_raw
        .as_ref()
        .and_then(|v| v.get("profileArn"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty())
}

/// Build per-model usage rows from a Kiro usage response (live or cached).
fn kiro_models_from_usage(usage: &KiroUsageResponse) -> Vec<QuotaModelUsage> {
    let mut models = Vec::new();
    for item in usage.usage_breakdown_list.iter().flatten() {
        let limit = item.usage_limit.unwrap_or(0.0);
        let used = item.current_usage.unwrap_or(0.0);
        let remaining = if limit > 0.0 {
            ((limit - used) / limit * 100.0).clamp(0.0, 100.0)
        } else {
            0.0
        };
        let reset = item.next_date_reset.and_then(format_reset_epoch);
        let name = item
            .display_name
            .clone()
            .unwrap_or_else(|| "Usage".to_string());
        models.push(model_usage(&name, remaining, reset));
    }
    models
}

/// Usage snapshot cached in the auth file — same data cpa-manager displays.
fn kiro_models_from_cache(auth: &KiroAuthFile) -> Vec<QuotaModelUsage> {
    if let Some(usage) = auth.kiro_usage_raw.as_ref() {
        let models = kiro_models_from_usage(usage);
        if !models.is_empty() {
            return models;
        }
    }
    if let Some(total) = auth.credits_total.filter(|t| *t > 0.0) {
        let used = auth.credits_used.unwrap_or(0.0);
        let remaining = ((total - used) / total * 100.0).clamp(0.0, 100.0);
        let reset = auth.usage_reset_at.and_then(format_reset_epoch);
        return vec![model_usage("Credit", remaining, reset)];
    }
    Vec::new()
}

fn fetch_kiro_one(agent: &ureq::Agent, path: &Path, filename: &str) -> Option<AccountQuota> {
    let raw = fs::read_to_string(path).ok()?;
    let auth: KiroAuthFile = serde_json::from_str(&raw).ok()?;
    let key = clean_filename(filename, "kiro-");
    let label = auth
        .email
        .clone()
        .filter(|email| !email.trim().is_empty())
        .unwrap_or_else(|| key.clone());

    let profile_arn = auth
        .profile_arn
        .clone()
        .filter(|s| !s.is_empty())
        .or_else(|| kiro_nested_profile_arn(&auth));

    let region = extract_kiro_region(profile_arn.as_deref())
        .or_else(|| auth.region.clone())
        .unwrap_or_else(|| KIRO_DEFAULT_REGION.to_string());
    let machine_id = kiro_machine_id(&auth);
    let user_agent = format!(
        "aws-sdk-js/1.0.0 ua/2.1 os/windows#10 lang/js md/nodejs#22.21.1 api/codewhispererruntime#1.0.0 m/N,E KiroIDE-{}-{}",
        KIRO_VERSION, machine_id
    );

    let mut url = format!(
        "https://q.{}.amazonaws.com/getUsageLimits?origin=AI_EDITOR&resourceType=AGENTIC_REQUEST",
        region
    );
    if let Some(arn) = profile_arn.as_deref().filter(|value| !value.is_empty()) {
        url.push_str(&format!("&profileArn={}", urlencoding::encode(arn)));
    }

    // Try the live usage endpoint, refreshing the token once on 401/403.
    let live = match call_kiro_usage(agent, &url, &auth.access_token, &user_agent) {
        Ok(u) => Some(u),
        Err(ureq::Error::Status(401, _)) | Err(ureq::Error::Status(403, _)) => auth
            .refresh_token
            .as_deref()
            .filter(|s| !s.is_empty())
            .and_then(|rt| try_refresh_kiro_token(agent, path, rt))
            .and_then(|new_at| call_kiro_usage(agent, &url, &new_at, &user_agent).ok()),
        Err(_) => None,
    };

    // Prefer fresh data; fall back to the snapshot cached in the auth file (what
    // cpa-manager displays) when the live call fails or returns nothing.
    let mut models = live
        .as_ref()
        .map(kiro_models_from_usage)
        .unwrap_or_default();
    if models.is_empty() {
        models = kiro_models_from_cache(&auth);
    }

    if models.is_empty() {
        return Some(AccountQuota {
            provider_id: "kiro".to_string(),
            account_label: label,
            account_key: key,
            is_forbidden: true,
            status_message: Some("需要重新授权".to_string()),
            models: Vec::new(),
        });
    }

    Some(AccountQuota {
        provider_id: "kiro".to_string(),
        account_label: label,
        account_key: key,
        is_forbidden: false,
        status_message: None,
        models,
    })
}

/// Extract the region from a CodeWhisperer profile ARN
/// (`arn:aws:codewhisperer:<region>:...`).
fn extract_kiro_region(profile_arn: Option<&str>) -> Option<String> {
    let arn = profile_arn?;
    let parts: Vec<&str> = arn.split(':').collect();
    if parts.len() >= 4
        && parts[0] == "arn"
        && parts[2] == "codewhisperer"
        && parts[3].contains('-')
    {
        Some(parts[3].to_string())
    } else {
        None
    }
}

/// Account-based machine id (SHA-256 hex of clientId/refreshToken).
fn kiro_machine_id(auth: &KiroAuthFile) -> String {
    let seed = auth
        .client_id
        .as_deref()
        .or(auth.refresh_token.as_deref())
        .unwrap_or("quotio-kiro");
    let mut hasher = Sha256::new();
    hasher.update(seed.as_bytes());
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{:02x}", byte))
        .collect()
}

#[derive(Debug, Deserialize)]
struct KiroAuthFile {
    #[serde(alias = "accessToken")]
    access_token: String,
    #[serde(default, alias = "refreshToken")]
    refresh_token: Option<String>,
    #[serde(default, alias = "clientId")]
    client_id: Option<String>,
    #[serde(default)]
    email: Option<String>,
    #[serde(default, alias = "profileArn")]
    profile_arn: Option<String>,
    #[serde(default)]
    region: Option<String>,
    // cockpit-tools-style files cache the last usage snapshot inline. We read it
    // as a fallback so an expired access_token still shows the real quota.
    #[serde(default)]
    kiro_auth_token_raw: Option<serde_json::Value>,
    #[serde(default)]
    kiro_usage_raw: Option<KiroUsageResponse>,
    #[serde(default)]
    credits_total: Option<f64>,
    #[serde(default)]
    credits_used: Option<f64>,
    #[serde(default)]
    usage_reset_at: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct KiroUsageResponse {
    #[serde(default, rename = "usageBreakdownList")]
    usage_breakdown_list: Option<Vec<KiroUsageBreakdown>>,
}

#[derive(Debug, Deserialize)]
struct KiroUsageBreakdown {
    #[serde(default, rename = "displayName")]
    display_name: Option<String>,
    #[serde(default, rename = "currentUsage")]
    current_usage: Option<f64>,
    #[serde(default, rename = "usageLimit")]
    usage_limit: Option<f64>,
    #[serde(default, rename = "nextDateReset")]
    next_date_reset: Option<f64>,
}

// ===================== GLM (BigModel) =====================

fn fetch_glm_quotas(agent: &ureq::Agent) -> Vec<AccountQuota> {
    let mut quotas = Vec::new();
    for (index, key) in read_glm_keys().into_iter().enumerate() {
        if let Some(quota) = fetch_glm_one(agent, &key, index) {
            quotas.push(quota);
        }
    }
    quotas
}

/// GLM uses API-key auth (not OAuth files). The user lists keys in
/// `~/.cli-proxy-api/glm-keys.json` as a JSON array of strings.
fn read_glm_keys() -> Vec<String> {
    let Some(dir) = auth_dir() else {
        return Vec::new();
    };
    let Ok(raw) = fs::read_to_string(dir.join("glm-keys.json")) else {
        return Vec::new();
    };
    serde_json::from_str::<Vec<String>>(&raw).unwrap_or_default()
}

fn fetch_glm_one(agent: &ureq::Agent, api_key: &str, index: usize) -> Option<AccountQuota> {
    let response = agent
        .get(GLM_QUOTA_URL)
        .set("Authorization", &format!("Bearer {}", api_key))
        .call();
    let parsed: GlmResponse = match response {
        Ok(resp) => resp.into_json().ok()?,
        Err(ureq::Error::Status(401, _)) | Err(ureq::Error::Status(403, _)) => {
            return Some(AccountQuota {
                provider_id: "glm".to_string(),
                account_label: format!("GLM #{}", index + 1),
                account_key: format!("glm-{}", index),
                is_forbidden: true,
                status_message: Some("密钥无效".to_string()),
                models: Vec::new(),
            });
        }
        Err(_) => return None,
    };

    let mut models = Vec::new();
    // data 缺失(200 但无内容)也按「健康、无额度数据」处理 → 落到下面的 Some(空),
    // 让恢复的号能被对账解除隔离,而不是 `?` 直接 None 掉出 quotas。
    for limit in parsed.data.map(|data| data.limits).unwrap_or_default() {
        let name = match limit.kind.as_deref() {
            Some("TOKENS_LIMIT") => "Tokens",
            Some("TIME_LIMIT") => "MCP Usage",
            _ => continue,
        };
        let remaining = 100.0 - limit.percentage.unwrap_or(0.0);
        let reset = limit.next_reset_time.and_then(format_reset_epoch);
        models.push(model_usage(name, remaining, reset));
    }

    // 探测成功但无额度条 = 健康、只是没有用量数据;返回 Some(空) 而非 None,
    // 让健康隔离的号在恢复后即便没拉到额度也能被对账确证健康、解除隔离。
    Some(AccountQuota {
        provider_id: "glm".to_string(),
        account_label: format!("GLM #{}", index + 1),
        account_key: format!("glm-{}", index),
        is_forbidden: false,
        status_message: None,
        models,
    })
}

#[derive(Debug, Deserialize)]
struct GlmResponse {
    #[serde(default)]
    data: Option<GlmData>,
}

#[derive(Debug, Deserialize)]
struct GlmData {
    #[serde(default)]
    limits: Vec<GlmLimit>,
}

#[derive(Debug, Deserialize)]
struct GlmLimit {
    #[serde(default, rename = "type")]
    kind: Option<String>,
    #[serde(default)]
    percentage: Option<f64>,
    #[serde(default, rename = "nextResetTime")]
    next_reset_time: Option<f64>,
}

// ===================== Trae (ByteDance) =====================

fn fetch_trae_quotas(agent: &ureq::Agent) -> Vec<AccountQuota> {
    fetch_trae_one(agent).into_iter().collect()
}

fn fetch_trae_one(agent: &ureq::Agent) -> Option<AccountQuota> {
    let auth = read_trae_auth()?;
    let token = auth.access_token.as_deref()?;
    let host = auth
        .api_host
        .as_deref()
        .filter(|value| !value.is_empty())
        .unwrap_or("https://api-sg-central.trae.ai");
    let url = format!("{}/trae/api/v1/pay/user_current_entitlement_list", host);

    let response = agent
        .post(&url)
        .set("Authorization", &format!("Cloud-IDE-JWT {}", token))
        .set("Content-Type", "application/json")
        .set("Accept", "application/json")
        .send_string("{\"require_usage\":true}");
    let parsed: TraeResponse = match response {
        Ok(resp) => resp.into_json().ok()?,
        Err(ureq::Error::Status(401, _)) | Err(ureq::Error::Status(403, _)) => {
            return Some(AccountQuota {
                provider_id: "trae".to_string(),
                account_label: auth.email.clone().unwrap_or_else(|| "Trae".to_string()),
                account_key: "trae".to_string(),
                is_forbidden: true,
                status_message: Some("需要重新登录".to_string()),
                models: Vec::new(),
            });
        }
        Err(_) => return None,
    };

    // 无 entitlement pack(200 但无订阅/内容)也按「健康、无额度数据」处理 → 落到下面
    // 的 Some(空),让恢复的号能被对账解除隔离,而不是 `?` 直接 None 掉出 quotas。
    let entitlement = parsed.user_entitlement_pack_list.as_ref().and_then(|packs| {
        packs
            .iter()
            .find(|pack| pack.status == Some(1))
            .or_else(|| packs.first())
    });
    let mut models = Vec::new();
    if let Some(entitlement) = entitlement {
        let quota = entitlement
            .entitlement_base_info
            .as_ref()
            .and_then(|info| info.quota.as_ref());
        let usage = entitlement.usage.as_ref();
        let reset = entitlement
            .entitlement_base_info
            .as_ref()
            .and_then(|info| info.end_time)
            .and_then(|seconds| format_reset_epoch(seconds as f64));
        push_trae_model(
            &mut models,
            "Premium Fast",
            quota.and_then(|q| q.premium_model_fast_request_limit),
            usage.and_then(|u| u.premium_model_fast_amount),
            &reset,
        );
        push_trae_model(
            &mut models,
            "Premium Slow",
            quota.and_then(|q| q.premium_model_slow_request_limit),
            usage.and_then(|u| u.premium_model_slow_amount),
            &reset,
        );
        push_trae_model(
            &mut models,
            "Advanced Model",
            quota.and_then(|q| q.advanced_model_request_limit),
            usage.and_then(|u| u.advanced_model_amount),
            &reset,
        );
        push_trae_model(
            &mut models,
            "Auto Completion",
            quota.and_then(|q| q.auto_completion_limit),
            usage.and_then(|u| u.auto_completion_amount),
            &reset,
        );
    }

    let label = auth
        .email
        .clone()
        .or_else(|| auth.username.clone())
        .unwrap_or_else(|| "Trae".to_string());
    // 探测成功但无额度条 = 健康、只是没有用量数据;返回 Some(空) 而非 None,
    // 让健康隔离的号在恢复后即便没拉到额度也能被对账确证健康、解除隔离。
    Some(AccountQuota {
        provider_id: "trae".to_string(),
        account_label: label,
        account_key: "trae".to_string(),
        is_forbidden: false,
        status_message: None,
        models,
    })
}

fn push_trae_model(
    models: &mut Vec<QuotaModelUsage>,
    name: &str,
    limit: Option<i64>,
    used: Option<i64>,
    reset: &Option<String>,
) {
    let limit = limit.unwrap_or(0);
    if limit <= 0 {
        return;
    }
    let used = used.unwrap_or(0);
    let remaining = (((limit - used) as f64 / limit as f64) * 100.0).clamp(0.0, 100.0);
    models.push(model_usage(name, remaining, reset.clone()));
}

struct TraeAuth {
    access_token: Option<String>,
    api_host: Option<String>,
    email: Option<String>,
    username: Option<String>,
}

/// Read Trae's IDE auth from `%APPDATA%/Trae/User/globalStorage/storage.json`.
fn read_trae_auth() -> Option<TraeAuth> {
    let base = std::env::var("APPDATA").ok()?;
    let path = std::path::Path::new(&base)
        .join("Trae")
        .join("User")
        .join("globalStorage")
        .join("storage.json");
    let raw = fs::read_to_string(path).ok()?;
    let storage: serde_json::Value = serde_json::from_str(&raw).ok()?;
    let auth_str = storage.get("iCubeAuthInfo://icube.cloudide")?.as_str()?;
    let auth: serde_json::Value = serde_json::from_str(auth_str).ok()?;
    let account = auth.get("account");
    Some(TraeAuth {
        access_token: auth
            .get("token")
            .and_then(|value| value.as_str())
            .map(String::from),
        api_host: auth
            .get("host")
            .and_then(|value| value.as_str())
            .map(String::from),
        email: account
            .and_then(|entry| entry.get("email"))
            .and_then(|value| value.as_str())
            .map(String::from),
        username: account
            .and_then(|entry| entry.get("username"))
            .and_then(|value| value.as_str())
            .map(String::from),
    })
}

#[derive(Debug, Deserialize)]
struct TraeResponse {
    #[serde(default)]
    user_entitlement_pack_list: Option<Vec<TraeEntitlement>>,
}

#[derive(Debug, Deserialize)]
struct TraeEntitlement {
    #[serde(default)]
    status: Option<i64>,
    #[serde(default)]
    entitlement_base_info: Option<TraeBaseInfo>,
    #[serde(default)]
    usage: Option<TraeUsage>,
}

#[derive(Debug, Deserialize)]
struct TraeBaseInfo {
    #[serde(default)]
    quota: Option<TraeQuota>,
    #[serde(default)]
    end_time: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct TraeQuota {
    #[serde(default)]
    advanced_model_request_limit: Option<i64>,
    #[serde(default)]
    auto_completion_limit: Option<i64>,
    #[serde(default)]
    premium_model_fast_request_limit: Option<i64>,
    #[serde(default)]
    premium_model_slow_request_limit: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct TraeUsage {
    #[serde(default)]
    advanced_model_amount: Option<i64>,
    #[serde(default)]
    auto_completion_amount: Option<i64>,
    #[serde(default)]
    premium_model_fast_amount: Option<i64>,
    #[serde(default)]
    premium_model_slow_amount: Option<i64>,
}
