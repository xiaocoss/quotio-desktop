//! Manages the **kiro-rs** sidecar — a standalone Rust server that exposes Kiro
//! (AWS CodeWhisperer) as an Anthropic-compatible API on a local port.
//!
//! CLIProxyAPI (the proxy core Quotio drives) has no CodeWhisperer support, so
//! it cannot route to Kiro on its own. We run kiro-rs alongside it and register
//! the sidecar as a `claude-api-key` provider in `config.yaml`. That lets the
//! existing per-key→pool binding (provider id `kiro`) gate Kiro just like any
//! other pool: a client key bound to `kiro` is written as `allowed-api-keys` on
//! the sidecar provider, and requests for Kiro models flow
//! `client → CLIProxyAPI → kiro-rs → CodeWhisperer`.
//!
//! Quotio owns the sidecar's lifecycle: it derives kiro-rs credentials from the
//! existing `~/.cli-proxy-api/kiro-*.json` files, starts/stops the process with
//! the proxy, and only runs it when at least one Kiro account exists.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use serde_json::{json, Value};
use uuid::Uuid;

/// Host kiro-rs binds to (loopback only — it is an internal hop).
pub const KIRO_SIDECAR_HOST: &str = "127.0.0.1";
/// Port kiro-rs listens on. CLIProxyAPI defaults to 28317; we sit next to it.
pub const KIRO_SIDECAR_PORT: u16 = 28319;
/// Built-in provider id the API-key bindings use for Kiro.
pub const KIRO_PROVIDER_ID: &str = "kiro";

/// Directory holding kiro-rs's generated `config.json` + `credentials.json`.
fn work_dir() -> PathBuf {
    quotio_platform::app_config_dir().join("kiro-rs")
}

/// Stable local api-key the sidecar uses to authenticate its caller
/// (CLIProxyAPI). Persisted so the value written into kiro-rs's config matches
/// the one written into CLIProxyAPI's `config.yaml` across restarts. Disk-backed
/// (not held in the struct) so the config writer can read it without `&mut`.
pub fn current_api_key() -> String {
    let path = work_dir().join("api-key");
    if let Ok(existing) = fs::read_to_string(&path) {
        let trimmed = existing.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    let key = format!("sk-kiro-{}", Uuid::new_v4().simple());
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let _ = fs::write(&path, &key);
    key
}

/// Map one Quotio `kiro-*.json` (cockpit-tools layout) into a kiro-rs credential
/// object (camelCase, as kiro-rs expects). Returns `None` when the file lacks a
/// refresh token — kiro-rs needs it to mint access tokens.
fn map_credential(file: &Value) -> Option<Value> {
    let raw = file.get("kiro_auth_token_raw");
    // 每个字段:先从顶层取(Kiro 客户端原生 / 用户直接粘的扁平布局,camelCase 或 snake_case),
    // 取不到再从 kiro_auth_token_raw 包裹层取(cockpit-tools 导出布局)。两种布局都要认——之前
    // 只认包裹层,扁平粘贴的凭据(顶层 refreshToken)会整个映射失败、kiro-rs 一个号都拿不到 →
    // 「导入成功却取不到模型」的真根因之一。
    let pick = |keys: &[&str]| -> Option<String> {
        keys.iter()
            .find_map(|k| {
                file.get(*k)
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
            })
            .or_else(|| {
                keys.iter().find_map(|k| {
                    raw.and_then(|r| r.get(*k))
                        .and_then(|v| v.as_str())
                        .filter(|s| !s.is_empty())
                })
            })
            .map(str::to_string)
    };

    let refresh_token = pick(&["refresh_token", "refreshToken"])?;

    let mut cred = serde_json::Map::new();
    cred.insert("refreshToken".into(), json!(refresh_token));
    // authMethod:归一到 kiro-rs 认的写法(它只认 idc / builder-id / iam,**不认 awsidc**;
    // 之前原样透传 "awsidc" 或默认 "social",组织账号就落到 social 刷新端点、token 刷不出来 →
    // 取不到模型)。源里没写就不塞,让 kiro-rs 按有无 clientId/clientSecret 自动判断(idc 凭据
    // 自带这俩);源里有就归一后写入。
    if let Some(method) = pick(&["authMethod", "auth_method"]) {
        cred.insert(
            "authMethod".into(),
            json!(canonicalize_kiro_auth_method(&method)),
        );
    }

    if let Some(expires_at) = pick(&["expiresAt", "expires_at"]) {
        cred.insert("expiresAt".into(), json!(expires_at));
    }
    if let Some(email) = pick(&["email"]) {
        cred.insert("email".into(), json!(email));
    }

    // profileArn lives in kiro_auth_token_raw.profileArn、顶层, or kiro_profile_raw.arn.
    let profile_arn = pick(&["profileArn", "profile_arn"]).or_else(|| {
        file.get("kiro_profile_raw")
            .and_then(|p| p.get("arn"))
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    });
    if let Some(arn) = profile_arn {
        cred.insert("profileArn".into(), json!(arn));
    }

    // 组织(IAM Identity Center)刷新必需:clientId / clientSecret / region。region 源里可能
    // 叫 region / idc_region / idcRegion / authRegion / auth_region,全部归到 kiro-rs 认的 region。
    if let Some(v) = pick(&["clientId", "client_id"]) {
        cred.insert("clientId".into(), json!(v));
    }
    if let Some(v) = pick(&["clientSecret", "client_secret"]) {
        cred.insert("clientSecret".into(), json!(v));
    }
    if let Some(v) = pick(&["region", "idc_region", "idcRegion", "authRegion", "auth_region"]) {
        cred.insert("region".into(), json!(v));
    }

    Some(Value::Object(cred))
}

/// 把各种「组织 / IAM Identity Center」的 authMethod 写法归一到 kiro-rs 认的 `idc`(它的
/// token_manager 只按 idc / builder-id / iam 走 AWS SSO OIDC 刷新端点,**不认 awsidc**——
/// Kiro 组织登录给的正是 awsidc)。`api_key` 归一到 `api_key`;其余原样返回。
fn canonicalize_kiro_auth_method(method: &str) -> &str {
    let m = method.trim();
    if m.eq_ignore_ascii_case("awsidc")
        || m.eq_ignore_ascii_case("aws-idc")
        || m.eq_ignore_ascii_case("idc")
        || m.eq_ignore_ascii_case("builder-id")
        || m.eq_ignore_ascii_case("iam")
        || m.eq_ignore_ascii_case("iam-identity-center")
        || m.eq_ignore_ascii_case("sso")
    {
        "idc"
    } else if m.eq_ignore_ascii_case("api_key") || m.eq_ignore_ascii_case("apikey") {
        "api_key"
    } else {
        m
    }
}

/// Read every `kiro-*.json` under `auth_dir` and map them to kiro-rs credentials.
fn collect_credentials(auth_dir: &Path) -> Vec<Value> {
    let mut creds = Vec::new();
    let Ok(entries) = fs::read_dir(auth_dir) else {
        return creds;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_lowercase();
        if !name.starts_with("kiro-") || !name.ends_with(".json") {
            continue;
        }
        let Ok(text) = fs::read_to_string(entry.path()) else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        if let Some(cred) = map_credential(&value) {
            creds.push(cred);
        }
    }
    creds
}

/// Number of routable Kiro accounts found under `auth_dir`.
pub fn kiro_account_count(auth_dir: &Path) -> usize {
    collect_credentials(auth_dir).len()
}

/// Write kiro-rs's `config.json` + `credentials.json`. We deliberately do NOT set
/// `proxyUrl`: both Kiro endpoints (`kiro.dev` token refresh and `q.*.amazonaws`
/// runtime) are reachable directly here, and routing kiro-rs's reqwest client
/// through the upstream proxy actually BREAKS the refresh ("error sending
/// request"), which then disables the single Kiro credential. Direct works.
fn write_files(api_key: &str, creds: &[Value]) -> std::io::Result<()> {
    let dir = work_dir();
    fs::create_dir_all(&dir)?;

    let mut config = serde_json::Map::new();
    config.insert("host".into(), json!(KIRO_SIDECAR_HOST));
    config.insert("port".into(), json!(KIRO_SIDECAR_PORT));
    config.insert("apiKey".into(), json!(api_key));
    config.insert("region".into(), json!("us-east-1"));
    config.insert("defaultEndpoint".into(), json!("ide"));
    fs::write(
        dir.join("config.json"),
        serde_json::to_string_pretty(&Value::Object(config))?,
    )?;
    fs::write(
        dir.join("credentials.json"),
        serde_json::to_string_pretty(&Value::Array(creds.to_vec()))?,
    )?;
    Ok(())
}

/// Locate the kiro-rs binary, keeping the managed copy in sync with the bundled
/// one. The bundled resource (its OWN `resources/kiro/<platform>` dir — NEVER the
/// proxy resource dir, whose resolver would grab any `.exe` as the proxy core) is
/// the source of truth: it's (re)staged into the work dir whenever it differs by
/// size from the managed copy, so an app upgrade shipping a newer kiro-rs replaces
/// the old one instead of being shadowed by it. Falls back to an existing managed
/// copy when there's no bundle (a platform without the asset) or the copy fails
/// (binary in use). Mirrors the proxy core's `prepare_managed_binary`.
fn resolve_binary() -> Option<PathBuf> {
    let name = if cfg!(windows) { "kiro-rs.exe" } else { "kiro-rs" };
    let managed = work_dir().join(name);
    let bundled = quotio_platform::kiro_resource_dir().join(name);

    if bundled.is_file() {
        let stale = !managed.is_file() || file_len(&bundled) != file_len(&managed);
        if stale {
            let _ = fs::create_dir_all(work_dir());
            if fs::copy(&bundled, &managed).is_ok() {
                // Tauri's resource bundling can drop the unix exec bit; restore it
                // so the staged sidecar is launchable on macOS/Linux. No-op on Win.
                let _ = crate::make_executable(&managed);
                return Some(managed);
            }
            // Copy failed (e.g. managed locked by a running sidecar) — use whatever
            // copy already exists.
            return if managed.is_file() { Some(managed) } else { Some(bundled) };
        }
        return Some(managed);
    }

    if managed.is_file() {
        return Some(managed);
    }
    None
}

/// Byte size of `path`, or 0 if unreadable — cheap "did the binary change?" check.
fn file_len(path: &Path) -> u64 {
    fs::metadata(path).map(|meta| meta.len()).unwrap_or(0)
}

/// Single-quote a YAML scalar (doubling embedded quotes) — safe for keys/URLs.
fn yaml_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

/// Models advertised for the kiro-rs sidecar. CLIProxyAPI only routes a model to
/// a `claude-api-key` provider when that model is listed (empty ⇒ no routing). We
/// deliberately list only the set a Kiro FREE account can actually run — Sonnet
/// 4.5 and Haiku 4.5. Listing models the account can't use (Opus, Sonnet 4.6) is
/// a footgun: the upstream returns INVALID_MODEL_ID, which cools down the whole
/// Kiro auth and breaks the supported models too. Expand this for paid accounts.
const DEFAULT_KIRO_MODELS: &[&str] = &[
    "claude-sonnet-4-5-20250929",
    "claude-sonnet-4-5-20250929-thinking",
    "claude-haiku-4-5-20251001",
    "claude-haiku-4-5-20251001-thinking",
];

/// Render the `claude-api-key` provider block that routes to the sidecar.
/// `bound_keys` are client api-keys bound to the `kiro` pool; when present they
/// become `allowed-api-keys`, restricting the sidecar to those keys.
pub fn provider_yaml(api_key: &str, bound_keys: &[String]) -> String {
    let base_url = format!("http://{KIRO_SIDECAR_HOST}:{KIRO_SIDECAR_PORT}");
    let mut out = String::from("\n# Kiro (managed by Quotio — kiro-rs sidecar)\nclaude-api-key:\n");
    out.push_str(&format!("  - api-key: {}\n", yaml_quote(api_key)));
    out.push_str(&format!("    base-url: {}\n", yaml_quote(&base_url)));
    // A Kiro account is a single credential with no fallback, so CLIProxyAPI
    // cooldown only hurts: one transient/quota error parks the whole pool as
    // "no auth available". Disable cooling for this provider — retry in place.
    out.push_str("    disable-cooling: true\n");
    // Bare model names (no prefix): the quotio-key-router scheduler plugin routes
    // by inbound api-key, so Kiro must be a candidate under its real model ids.
    out.push_str("    models:\n");
    for model in DEFAULT_KIRO_MODELS {
        out.push_str(&format!("      - name: {}\n", yaml_quote(model)));
    }
    if !bound_keys.is_empty() {
        out.push_str("    allowed-api-keys:\n");
        for key in bound_keys {
            out.push_str(&format!("      - {}\n", yaml_quote(key)));
        }
    }
    out
}

/// Owns the kiro-rs child process. Lives inside the proxy lifecycle so the
/// sidecar starts/stops together with the core.
#[derive(Default)]
pub struct KiroSidecar {
    child: Option<Child>,
    active: bool,
}

impl KiroSidecar {
    /// Whether the sidecar is currently meant to be running (≥1 Kiro account).
    pub fn is_active(&self) -> bool {
        self.active
    }

    /// Refresh kiro-rs credentials from `auth_dir` and (re)start it when at least
    /// one Kiro account exists. Stops the sidecar when none remain.
    pub fn sync_and_start(&mut self, auth_dir: &Path) {
        let creds = collect_credentials(auth_dir);
        if creds.is_empty() {
            self.stop();
            self.active = false;
            return;
        }
        self.active = true;
        let _ = write_files(&current_api_key(), &creds);

        // Already running — the refreshed credential file is picked up on its own
        // reload; no need to bounce the process.
        if self.child.is_some() {
            return;
        }
        let Some(binary) = resolve_binary() else {
            return;
        };
        let dir = work_dir();
        let mut command = Command::new(&binary);
        command
            .arg("--config")
            .arg(dir.join("config.json"))
            .arg("--credentials")
            .arg(dir.join("credentials.json"))
            .current_dir(binary.parent().unwrap_or_else(|| Path::new(".")))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        }
        if let Ok(child) = command.spawn() {
            self.child = Some(child);
        }
    }

    /// Terminate the sidecar process if running.
    pub fn stop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_social_account_to_kiro_rs_credential() {
        // Mirrors the real cockpit-tools kiro-*.json layout.
        let file = json!({
            "refresh_token": "rt-top-level",
            "email": "user@example.com",
            "kiro_auth_token_raw": {
                "refreshToken": "rt-nested",
                "authMethod": "social",
                "expiresAt": "2026-06-18T17:07:40.000Z",
                "profileArn": "arn:aws:codewhisperer:us-east-1:123:profile/ABC"
            },
            "kiro_profile_raw": { "arn": "arn:aws:codewhisperer:us-east-1:123:profile/ABC" },
            "type": "kiro"
        });
        let cred = map_credential(&file).expect("should map");
        assert_eq!(cred["refreshToken"], json!("rt-top-level"));
        assert_eq!(cred["authMethod"], json!("social"));
        assert_eq!(cred["email"], json!("user@example.com"));
        assert_eq!(cred["expiresAt"], json!("2026-06-18T17:07:40.000Z"));
        assert_eq!(
            cred["profileArn"],
            json!("arn:aws:codewhisperer:us-east-1:123:profile/ABC")
        );
    }

    #[test]
    fn omits_auth_method_when_absent_so_kiro_rs_auto_detects() {
        let file = json!({
            "kiro_auth_token_raw": { "refreshToken": "rt-nested" }
        });
        let cred = map_credential(&file).expect("should map from nested token");
        assert_eq!(cred["refreshToken"], json!("rt-nested"));
        // raw 没写 authMethod → 不塞,交给 kiro-rs 按有无 clientId/clientSecret 自动判断(无 → social)。
        assert!(cred.as_object().unwrap().get("authMethod").is_none());
    }

    #[test]
    fn normalizes_org_awsidc_auth_method_to_idc() {
        // 组织账号(AWS IAM Identity Center)导入的凭据:authMethod=awsidc + idc 刷新所需字段。
        let file = json!({
            "kiro_auth_token_raw": {
                "refreshToken": "rt",
                "authMethod": "awsidc",
                "clientId": "cid",
                "clientSecret": "csec",
                "region": "us-east-1"
            }
        });
        let cred = map_credential(&file).expect("should map");
        // awsidc → idc,否则 kiro-rs 落到 social 刷新端点、组织账号取不到模型。
        assert_eq!(cred["authMethod"], json!("idc"));
        assert_eq!(cred["clientId"], json!("cid"));
        assert_eq!(cred["clientSecret"], json!("csec"));
        assert_eq!(cred["region"], json!("us-east-1"));
    }

    #[test]
    fn maps_flat_top_level_camelcase_layout() {
        // Kiro 客户端原生 / 用户直接粘的扁平布局:没有 kiro_auth_token_raw 包裹层,
        // 字段全在顶层 camelCase。之前只认包裹层 → 这种 JSON 整个映射失败 → 取不到模型。
        let file = json!({
            "refreshToken": "rt-flat",
            "authMethod": "idc",
            "clientId": "cid",
            "clientSecret": "csec",
            "region": "eu-west-1",
            "expiresAt": "2026-07-01T00:00:00.000Z",
            "email": "flat@example.com",
            "type": "kiro"
        });
        let cred = map_credential(&file).expect("flat layout should map");
        assert_eq!(cred["refreshToken"], json!("rt-flat"));
        assert_eq!(cred["authMethod"], json!("idc"));
        assert_eq!(cred["clientId"], json!("cid"));
        assert_eq!(cred["clientSecret"], json!("csec"));
        assert_eq!(cred["region"], json!("eu-west-1"));
        assert_eq!(cred["email"], json!("flat@example.com"));
    }

    #[test]
    fn reads_region_from_idc_region_alias() {
        // 组织凭据的 region 可能存成 idc_region(cockpit-tools 回调注入的写法);
        // kiro-rs 只认 region/authRegion,必须归一,否则 IdC 刷新缺 region。
        let file = json!({
            "refreshToken": "rt",
            "authMethod": "awsidc",
            "clientId": "cid",
            "clientSecret": "csec",
            "idc_region": "us-east-1"
        });
        let cred = map_credential(&file).expect("should map");
        assert_eq!(cred["authMethod"], json!("idc"));
        assert_eq!(cred["region"], json!("us-east-1"));
    }

    #[test]
    fn rejects_account_without_refresh_token() {
        let file = json!({ "email": "no-token@example.com" });
        assert!(map_credential(&file).is_none());
    }

    #[test]
    fn provider_yaml_includes_allowed_keys_when_bound() {
        let yaml = provider_yaml("sk-kiro-abc", &["sk-client-1".to_string()]);
        assert!(yaml.contains("claude-api-key:"));
        assert!(yaml.contains("base-url: 'http://127.0.0.1:28319'"));
        assert!(yaml.contains("allowed-api-keys:"));
        assert!(yaml.contains("- 'sk-client-1'"));
    }

    #[test]
    fn provider_yaml_omits_allowed_keys_when_unbound() {
        let yaml = provider_yaml("sk-kiro-abc", &[]);
        assert!(yaml.contains("api-key: 'sk-kiro-abc'"));
        assert!(!yaml.contains("allowed-api-keys"));
    }

    #[test]
    fn provider_yaml_declares_models_for_routing() {
        // CLIProxyAPI won't route to a claude-api-key provider with no models.
        let yaml = provider_yaml("sk-kiro-abc", &[]);
        assert!(yaml.contains("models:"));
        assert!(yaml.contains("- name: 'claude-sonnet-4-5-20250929'"));
        assert!(yaml.contains("- name: 'claude-haiku-4-5-20251001'"));
    }

}
