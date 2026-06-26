use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use rand::Rng;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::Write;
use std::net::TcpListener;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthStartResponse {
    pub login_id: String,
    pub auth_url: String,
    /// Device-flow only: user code to display.
    #[serde(default)]
    pub user_code: String,
    /// Device-flow only: the verification URI for the user.
    #[serde(default)]
    pub verification_uri: String,
    pub provider_id: String,
    pub flow: OAuthFlowKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthCompleteResponse {
    pub status: String,
    pub error: Option<String>,
    pub provider_id: String,
    pub account_email: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OAuthFlowKind {
    AuthorizationCode,
    DeviceCode,
}

// ---------------------------------------------------------------------------
// Per-provider OAuth configuration
// ---------------------------------------------------------------------------

struct AuthCodeConfig {
    client_id: &'static str,
    client_secret: Option<&'static str>,
    auth_endpoint: &'static str,
    token_endpoint: &'static str,
    scopes: &'static str,
    callback_path: &'static str,
    fixed_port: Option<u16>,
    use_pkce: bool,
    extra_auth_params: &'static [(&'static str, &'static str)],
}

struct DeviceCodeConfig {
    client_id: &'static str,
    device_code_endpoint: &'static str,
    token_endpoint: &'static str,
    scopes: &'static str,
    accept_json: bool,
}

enum ProviderOAuthConfig {
    AuthorizationCode(AuthCodeConfig),
    DeviceCode(DeviceCodeConfig),
}

fn provider_config(provider_id: &str) -> Option<ProviderOAuthConfig> {
    match provider_id {
        "codex" => Some(ProviderOAuthConfig::AuthorizationCode(AuthCodeConfig {
            client_id: "app_EMoamEEZ73f0CkXaXp7hrann",
            client_secret: None,
            auth_endpoint: "https://auth.openai.com/oauth/authorize",
            token_endpoint: "https://auth.openai.com/oauth/token",
            scopes: "openid profile email offline_access",
            callback_path: "/auth/callback",
            fixed_port: Some(1455),
            use_pkce: true,
            extra_auth_params: &[("originator", "codex_vscode")],
        })),
        "claude" => Some(ProviderOAuthConfig::AuthorizationCode(AuthCodeConfig {
            client_id: "app_EMoamEEZ73f0CkXaXp7hrann",
            client_secret: None,
            auth_endpoint: "https://auth.openai.com/oauth/authorize",
            token_endpoint: "https://auth.openai.com/oauth/token",
            scopes: "openid profile email offline_access",
            callback_path: "/auth/callback",
            fixed_port: Some(1455),
            use_pkce: true,
            extra_auth_params: &[("originator", "codex_vscode")],
        })),
        // gemini-cli uses Google's well-known installed-app OAuth credential.
        // It is NOT hardcoded in source — supply it at build time via env vars
        // QUOTIO_GEMINI_CLIENT_ID / QUOTIO_GEMINI_CLIENT_SECRET (baked in by
        // `option_env!`). If absent at build time, gemini-cli OAuth is unavailable.
        "gemini-cli" => option_env!("QUOTIO_GEMINI_CLIENT_ID").map(|client_id| {
            ProviderOAuthConfig::AuthorizationCode(AuthCodeConfig {
                client_id,
                client_secret: option_env!("QUOTIO_GEMINI_CLIENT_SECRET"),
                auth_endpoint: "https://accounts.google.com/o/oauth2/v2/auth",
                token_endpoint: "https://oauth2.googleapis.com/token",
                scopes: "openid https://www.googleapis.com/auth/cloud-platform https://www.googleapis.com/auth/userinfo.email https://www.googleapis.com/auth/userinfo.profile",
                callback_path: "/oauth2callback",
                fixed_port: None,
                use_pkce: false,
                extra_auth_params: &[("access_type", "offline"), ("prompt", "consent")],
            })
        }),
        "kiro" => Some(ProviderOAuthConfig::AuthorizationCode(AuthCodeConfig {
            client_id: "",
            client_secret: None,
            auth_endpoint: "https://app.kiro.dev/signin",
            token_endpoint: "https://prod.us-east-1.auth.desktop.kiro.dev/oauth/token",
            scopes: "",
            callback_path: "/oauth/callback",
            fixed_port: None,
            use_pkce: true,
            extra_auth_params: &[("redirect_from", "KiroIDE"), ("code_challenge_method", "S256")],
        })),
        "github-copilot" => Some(ProviderOAuthConfig::DeviceCode(DeviceCodeConfig {
            client_id: "01ab8ac9400c4e429b23",
            device_code_endpoint: "https://github.com/login/device/code",
            token_endpoint: "https://github.com/login/oauth/access_token",
            scopes: "read:user user:email",
            accept_json: true,
        })),
        "qwen" => Some(ProviderOAuthConfig::DeviceCode(DeviceCodeConfig {
            client_id: "e883ade2-e6e3-4d6d-adf7-f92ceff5fdcb",
            device_code_endpoint: "https://openapi.qoder.sh/api/v1/deviceToken/register",
            token_endpoint: "https://openapi.qoder.sh/api/v1/deviceToken/poll",
            scopes: "",
            accept_json: true,
        })),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Global state
// ---------------------------------------------------------------------------

#[allow(dead_code)]
#[derive(Debug, Clone)]
struct PendingOAuth {
    login_id: String,
    provider_id: String,
    flow: OAuthFlowKind,
    // Auth-code flow fields
    code_verifier: Option<String>,
    redirect_uri: Option<String>,
    state_token: Option<String>,
    callback_port: Option<u16>,
    auth_url: String,
    code: Option<String>,
    // Device-code flow fields
    device_code: Option<String>,
    user_code: Option<String>,
    verification_uri: Option<String>,
    interval_seconds: u64,
    /// 设备码流:下次允许真正轮询服务器的最早 unix 秒(节流 + slow_down 退避用)。
    /// 0 = 立即可轮询。前端固定 ~2s 调一次,这里据此跳过过早的轮询。
    next_poll_at: i64,
    // Shared
    expires_at: i64,
    error: Option<String>,
    completed: bool,
    // Config snapshot for token exchange
    client_id: String,
    client_secret: Option<String>,
    token_endpoint: String,
    use_pkce: bool,
}

static PENDING_OAUTH: OnceLock<Mutex<Option<PendingOAuth>>> = OnceLock::new();

fn get_state() -> &'static Mutex<Option<PendingOAuth>> {
    PENDING_OAUTH.get_or_init(|| Mutex::new(None))
}

fn now_ts() -> i64 {
    chrono::Utc::now().timestamp()
}

const OAUTH_TIMEOUT_SECONDS: i64 = 300;

fn native_oauth_log_path() -> std::path::PathBuf {
    quotio_platform::app_logs_dir().join("native-oauth.log")
}

fn log_oauth_event(provider_id: &str, login_id: Option<&str>, stage: &str, detail: &str) {
    let safe_detail = detail.replace(['\r', '\n'], " ");
    let login = login_id.unwrap_or("-");
    let line = format!(
        "{} provider={} login_id={} stage={} {}\n",
        chrono::Utc::now().to_rfc3339(),
        provider_id,
        login,
        stage,
        safe_detail
    );

    eprintln!(
        "[OAuth] provider={} login_id={} stage={} {}",
        provider_id, login, stage, safe_detail
    );

    let path = native_oauth_log_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = file.write_all(line.as_bytes());
    }
}

fn with_log_hint(message: &str) -> String {
    format!(
        "{}（详情见日志：{}）",
        message,
        native_oauth_log_path().display()
    )
}

fn complete_error_response(
    login_id: &str,
    provider_id: &str,
    stage: &str,
    message: String,
) -> OAuthCompleteResponse {
    log_oauth_event(provider_id, Some(login_id), stage, &message);
    let error = with_log_hint(&message);
    if let Ok(mut guard) = get_state().lock() {
        if let Some(s) = guard.as_mut() {
            if s.login_id == login_id {
                s.error = Some(error.clone());
            }
        }
    }
    OAuthCompleteResponse {
        status: "error".to_string(),
        error: Some(error),
        provider_id: provider_id.to_string(),
        account_email: None,
    }
}

// ---------------------------------------------------------------------------
// PKCE helpers
// ---------------------------------------------------------------------------

fn generate_token() -> String {
    let mut rng = rand::thread_rng();
    let bytes: Vec<u8> = (0..32).map(|_| rng.gen::<u8>()).collect();
    URL_SAFE_NO_PAD.encode(bytes)
}

fn generate_code_challenge(verifier: &str) -> String {
    let hash = Sha256::digest(verifier.as_bytes());
    URL_SAFE_NO_PAD.encode(hash)
}

// ---------------------------------------------------------------------------
// Callback server (authorization-code flow)
// ---------------------------------------------------------------------------

fn oauth_success_html() -> &'static str {
    "<html><head><meta charset='utf-8'></head>\
    <body style='margin:0;min-height:100vh;display:flex;align-items:center;justify-content:center;\
    background:linear-gradient(135deg,#667eea 0%,#764ba2 100%);font-family:system-ui,sans-serif'>\
    <div style='text-align:center;color:#fff'>\
    <div style='font-size:64px;margin-bottom:16px'>&#9989;</div>\
    <h1 style='font-size:32px;font-weight:700;margin:0 0 12px'>已收到授权回调</h1>\
    <p style='font-size:16px;opacity:.85;margin:0'>请返回 Quotio 等待账号保存完成</p>\
    </div>\
    <script>setTimeout(function(){window.close()},3000)</script>\
    </body></html>"
}

fn oauth_error_html(msg: &str) -> String {
    format!(
        "<html><head><meta charset='utf-8'></head>\
        <body style='margin:0;min-height:100vh;display:flex;align-items:center;justify-content:center;\
        background:linear-gradient(135deg,#e53e3e 0%,#9b2c2c 100%);font-family:system-ui,sans-serif'>\
        <div style='text-align:center;color:#fff'>\
        <div style='font-size:64px;margin-bottom:16px'>&#10060;</div>\
        <h1 style='font-size:32px;font-weight:700;margin:0 0 12px'>授权失败</h1>\
        <p style='font-size:16px;opacity:.85;margin:0'>{}</p>\
        </div>\
        </body></html>",
        msg
    )
}

fn html_response(
    body: &str,
    status: u16,
) -> tiny_http::Response<std::io::Cursor<Vec<u8>>> {
    let bytes = body.as_bytes().to_vec();
    let len = bytes.len();
    let header = tiny_http::Header::from_bytes(
        &b"Content-Type"[..],
        &b"text/html; charset=utf-8"[..],
    )
    .unwrap();
    tiny_http::Response::new(
        tiny_http::StatusCode(status),
        vec![header],
        std::io::Cursor::new(bytes),
        Some(len),
        None,
    )
}

fn find_available_port(fixed: Option<u16>) -> Result<u16, String> {
    if let Some(port) = fixed {
        TcpListener::bind(("127.0.0.1", port))
            .map_err(|_| format!("端口 {} 被占用，请关闭占用进程后重试", port))?;
        // We only checked availability; the actual listener is started later.
        return Ok(port);
    }
    // Dynamic: try a few candidates, then fall back to OS assignment.
    for port in [3128, 4649, 6588, 8008, 9091, 49153, 50153] {
        if TcpListener::bind(("127.0.0.1", port)).is_ok() {
            return Ok(port);
        }
    }
    let listener = TcpListener::bind("127.0.0.1:0")
        .map_err(|e| format!("无法绑定本地端口: {}", e))?;
    listener
        .local_addr()
        .map(|a| a.port())
        .map_err(|e| format!("无法获取本地端口: {}", e))
}

fn start_callback_server(
    port: u16,
    expected_state: String,
    expected_login_id: String,
    callback_path: String,
) {
    std::thread::spawn(move || {
        let server = match tiny_http::Server::http(format!("127.0.0.1:{}", port)) {
            Ok(s) => {
                log_oauth_event(
                    "unknown",
                    Some(&expected_login_id),
                    "callback_listening",
                    &format!("listening on 127.0.0.1:{}{}", port, callback_path),
                );
                s
            }
            Err(e) => {
                eprintln!("[OAuth] 回调服务启动失败: {}", e);
                log_oauth_event(
                    "unknown",
                    Some(&expected_login_id),
                    "callback_start_failed",
                    &format!("回调服务启动失败: {}", e),
                );
                if let Ok(mut guard) = get_state().lock() {
                    if let Some(s) = guard.as_mut() {
                        if s.login_id == expected_login_id {
                            s.error = Some(format!("回调服务启动失败: {}", e));
                        }
                    }
                }
                return;
            }
        };

        let deadline = std::time::Instant::now() + Duration::from_secs(OAUTH_TIMEOUT_SECONDS as u64);

        loop {
            if std::time::Instant::now() > deadline {
                if let Ok(mut guard) = get_state().lock() {
                    if let Some(s) = guard.as_mut() {
                        if s.login_id == expected_login_id && s.code.is_none() && s.error.is_none() {
                            s.error = Some("OAuth 授权超时，请重试。".to_string());
                        }
                    }
                }
                break;
            }

            // Check if login was cancelled or completed.
            {
                let guard = get_state().lock().ok();
                match guard.as_ref().and_then(|g| g.as_ref()) {
                    Some(s) if s.login_id != expected_login_id => break,
                    Some(s) if s.code.is_some() || s.error.is_some() || s.completed => break,
                    None => break,
                    _ => {}
                }
            }

            let request = match server.try_recv() {
                Ok(Some(r)) => r,
                _ => {
                    std::thread::sleep(Duration::from_millis(150));
                    continue;
                }
            };

            let raw_url = request.url().to_string();
            let (path, query) = match raw_url.split_once('?') {
                Some((p, q)) => (p, q),
                None => (raw_url.as_str(), ""),
            };
            log_oauth_event(
                "unknown",
                Some(&expected_login_id),
                "callback_received",
                &format!("path={}", path),
            );

            // Only handle the expected callback path.
            if path != callback_path && path != "/signin/callback" {
                let _ = request.respond(
                    tiny_http::Response::from_string("Not Found")
                        .with_status_code(tiny_http::StatusCode(404)),
                );
                continue;
            }

            let params = parse_query(query);

            // Check for error in callback.
            if let Some(err) = params.get("error") {
                let desc = params.get("error_description").cloned().unwrap_or_default();
                let msg = if desc.is_empty() {
                    format!("授权失败: {}", err)
                } else {
                    format!("授权失败: {} ({})", err, desc)
                };
                if let Ok(mut guard) = get_state().lock() {
                    if let Some(s) = guard.as_mut() {
                        if s.login_id == expected_login_id {
                            log_oauth_event(&s.provider_id, Some(&expected_login_id), "callback_error", &msg);
                            s.error = Some(msg.clone());
                        }
                    }
                }
                let _ = request.respond(html_response(&oauth_error_html(&msg), 400));
                break;
            }

            // Validate state.
            let cb_state = params.get("state").cloned().unwrap_or_default();
            if cb_state != expected_state {
                let msg = "OAuth state 校验失败，请重新授权。";
                if let Ok(mut guard) = get_state().lock() {
                    if let Some(s) = guard.as_mut() {
                        if s.login_id == expected_login_id {
                            log_oauth_event(&s.provider_id, Some(&expected_login_id), "callback_state_mismatch", msg);
                            s.error = Some(msg.to_string());
                        }
                    }
                }
                let _ = request.respond(html_response(&oauth_error_html(msg), 400));
                break;
            }

            // Extract code.
            let code = params.get("code").cloned().unwrap_or_default();
            if code.is_empty() {
                let msg = "回调缺少授权 code。";
                if let Ok(mut guard) = get_state().lock() {
                    if let Some(s) = guard.as_mut() {
                        if s.login_id == expected_login_id {
                            log_oauth_event(&s.provider_id, Some(&expected_login_id), "callback_missing_code", msg);
                            s.error = Some(msg.to_string());
                        }
                    }
                }
                let _ = request.respond(html_response(&oauth_error_html(msg), 400));
                break;
            }

            // Store code.
            if let Ok(mut guard) = get_state().lock() {
                if let Some(s) = guard.as_mut() {
                    if s.login_id == expected_login_id {
                        log_oauth_event(&s.provider_id, Some(&expected_login_id), "callback_code_received", "authorization code received; waiting for token exchange");
                        s.code = Some(code);
                    }
                }
            }

            let _ = request.respond(html_response(&oauth_success_html(), 200));
            break;
        }
    });
}

fn parse_query(query: &str) -> HashMap<String, String> {
    query
        .split('&')
        .filter_map(|pair| {
            let mut parts = pair.splitn(2, '=');
            let key = parts.next()?.trim();
            if key.is_empty() {
                return None;
            }
            let value = urlencoding::decode(parts.next().unwrap_or(""))
                .unwrap_or_default()
                .into_owned();
            Some((key.to_string(), value))
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Token exchange
// ---------------------------------------------------------------------------

/// 共享带超时的 HTTP agent。裸 `ureq::post/get` 用的默认 agent 既无 connect 也无
/// read 超时,网络半开 / 被中间代理强制断开(wsarecv)时会永久卡住登录线程,连
/// cancel 都打不断。所有 OAuth 出站请求统一走它,与 management.rs 的做法一致。
fn http_agent() -> &'static ureq::Agent {
    static AGENT: OnceLock<ureq::Agent> = OnceLock::new();
    AGENT.get_or_init(|| {
        ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(15))
            .timeout_read(Duration::from_secs(30))
            .build()
    })
}

/// Token 交换的失败分类:决定前端是「继续轮询重试」还是「停在永久错误」。
enum TokenExchangeError {
    /// 瞬时(5xx / 连接断开 / 读超时):授权 code 未被消费,应在有效期内重试。
    Retryable(String),
    /// 永久(4xx invalid_grant、响应解析失败等):置错并停止。
    Permanent(String),
}

fn exchange_auth_code(pending: &PendingOAuth) -> Result<serde_json::Value, TokenExchangeError> {
    log_oauth_event(
        &pending.provider_id,
        Some(&pending.login_id),
        "token_exchange_start",
        &format!("endpoint={}", pending.token_endpoint),
    );
    let code = pending
        .code
        .as_deref()
        .ok_or_else(|| TokenExchangeError::Permanent("缺少授权 code".to_string()))?;
    let redirect_uri = pending
        .redirect_uri
        .as_deref()
        .ok_or_else(|| TokenExchangeError::Permanent("缺少 redirect_uri".to_string()))?;

    let mut params: Vec<(&str, &str)> = vec![
        ("grant_type", "authorization_code"),
        ("code", code),
        ("redirect_uri", redirect_uri),
        ("client_id", &pending.client_id),
    ];

    let verifier_str;
    if pending.use_pkce {
        if let Some(v) = &pending.code_verifier {
            verifier_str = v.clone();
            params.push(("code_verifier", &verifier_str));
        }
    }

    let secret_str;
    if let Some(s) = &pending.client_secret {
        secret_str = s.clone();
        params.push(("client_secret", &secret_str));
    }

    let body = params
        .iter()
        .map(|(k, v)| format!("{}={}", urlencoding::encode(k), urlencoding::encode(v)))
        .collect::<Vec<_>>()
        .join("&");

    let is_kiro = pending.provider_id == "kiro";

    let response = if is_kiro {
        http_agent()
            .post(&pending.token_endpoint)
            .set("Content-Type", "application/json")
            .send_string(
                &serde_json::json!({
                    "code": code,
                    "code_verifier": pending.code_verifier.as_deref().unwrap_or(""),
                    "redirect_uri": redirect_uri
                })
                .to_string(),
            )
    } else {
        http_agent()
            .post(&pending.token_endpoint)
            .set("Content-Type", "application/x-www-form-urlencoded")
            .send_string(&body)
    };

    let response = match response {
        Ok(resp) => resp,
        // 5xx:上游瞬时故障,授权 code 未被消费 → 可重试。
        Err(ureq::Error::Status(status, _)) if status >= 500 => {
            return Err(TokenExchangeError::Retryable(format!(
                "Token 交换上游返回 {status},稍后重试"
            )));
        }
        // 4xx(invalid_grant 等):永久失败,读出错误体便于诊断。
        Err(ureq::Error::Status(status, resp)) => {
            let detail = resp.into_string().unwrap_or_default();
            return Err(TokenExchangeError::Permanent(format!(
                "Token 交换失败({status}): {detail}"
            )));
        }
        // 传输层错误(连接被拒/被强制断开/超时):瞬时 → 可重试。
        Err(err @ ureq::Error::Transport(_)) => {
            return Err(TokenExchangeError::Retryable(format!(
                "Token 交换连接失败: {err}"
            )));
        }
    };
    let text = response
        .into_string()
        .map_err(|e| TokenExchangeError::Retryable(format!("读取 Token 响应失败: {}", e)))?;
    let mut token: serde_json::Value = serde_json::from_str(&text)
        .map_err(|e| TokenExchangeError::Permanent(format!("解析 Token 响应失败: {}", e)))?;
    log_oauth_event(
        &pending.provider_id,
        Some(&pending.login_id),
        "token_exchange_success",
        "token response received and parsed",
    );

    // Unwrap { "data": { ... } } wrapper if present (Kiro).
    if let Some(data) = token
        .as_object_mut()
        .and_then(|o| o.remove("data"))
        .filter(|v| v.is_object())
    {
        token = data;
    }

    Ok(token)
}

// ---------------------------------------------------------------------------
// Device-code flow helpers
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct DeviceCodeResponse {
    device_code: Option<String>,
    user_code: Option<String>,
    verification_uri: Option<String>,
    verification_uri_complete: Option<String>,
    expires_in: Option<u64>,
    interval: Option<u64>,
    // Qwen-style fields
    #[serde(rename = "deviceCode")]
    device_code_alt: Option<String>,
    #[serde(rename = "userCode")]
    user_code_alt: Option<String>,
    #[serde(rename = "verificationUri")]
    verification_uri_alt: Option<String>,
    #[serde(rename = "verificationUriComplete")]
    verification_uri_complete_alt: Option<String>,
    #[serde(rename = "expiresIn")]
    expires_in_alt: Option<u64>,
}

impl DeviceCodeResponse {
    fn device_code(&self) -> Option<&str> {
        self.device_code
            .as_deref()
            .or(self.device_code_alt.as_deref())
    }
    fn user_code(&self) -> Option<&str> {
        self.user_code
            .as_deref()
            .or(self.user_code_alt.as_deref())
    }
    fn verification_uri(&self) -> Option<&str> {
        self.verification_uri
            .as_deref()
            .or(self.verification_uri_alt.as_deref())
    }
    fn expires_in(&self) -> u64 {
        self.expires_in.or(self.expires_in_alt).unwrap_or(300)
    }
}

#[derive(Debug, Deserialize)]
struct DeviceTokenResponse {
    access_token: Option<String>,
    error: Option<String>,
    error_description: Option<String>,
    // Qwen-style
    #[serde(rename = "accessToken")]
    access_token_alt: Option<String>,
    token_type: Option<String>,
    scope: Option<String>,
    #[serde(flatten)]
    extra: HashMap<String, serde_json::Value>,
}

fn request_device_code(config: &DeviceCodeConfig) -> Result<DeviceCodeResponse, String> {
    let mut req = http_agent().post(config.device_code_endpoint);
    if config.accept_json {
        req = req.set("Accept", "application/json");
    }

    let mut form_params = vec![("client_id", config.client_id)];
    if !config.scopes.is_empty() {
        form_params.push(("scope", config.scopes));
    }

    let body = form_params
        .iter()
        .map(|(k, v)| format!("{}={}", urlencoding::encode(k), urlencoding::encode(v)))
        .collect::<Vec<_>>()
        .join("&");

    let response = req
        .set("Content-Type", "application/x-www-form-urlencoded")
        .send_string(&body)
        .map_err(|e| format!("请求 device code 失败: {}", e))?;

    let text = response
        .into_string()
        .map_err(|e| format!("读取 device code 响应失败: {}", e))?;
    serde_json::from_str(&text).map_err(|e| format!("解析 device code 响应失败: {}", e))
}

/// 设备码轮询结果:拿到 token / 仍在等待授权 / 服务器要求放慢(slow_down)。
enum DevicePoll {
    Token(serde_json::Value),
    Pending,
    SlowDown,
}

fn poll_device_token(pending: &PendingOAuth) -> Result<DevicePoll, String> {
    let device_code = pending
        .device_code
        .as_deref()
        .ok_or_else(|| "缺少 device_code".to_string())?;

    let req = http_agent()
        .post(&pending.token_endpoint)
        .set("Accept", "application/json")
        .set("Content-Type", "application/x-www-form-urlencoded");

    let body = if pending.provider_id == "qwen" {
        format!("deviceCode={}", urlencoding::encode(device_code))
    } else {
        format!(
            "client_id={}&device_code={}&grant_type={}",
            urlencoding::encode(&pending.client_id),
            urlencoding::encode(device_code),
            urlencoding::encode("urn:ietf:params:oauth:grant-type:device_code")
        )
    };

    let response = req
        .send_string(&body)
        .map_err(|e| format!("轮询 device token 失败: {}", e))?;

    let text = response
        .into_string()
        .map_err(|e| format!("读取 device token 响应失败: {}", e))?;
    let parsed: DeviceTokenResponse =
        serde_json::from_str(&text).map_err(|e| format!("解析 device token 响应失败: {}", e))?;

    if let Some(error) = &parsed.error {
        match error.as_str() {
            "authorization_pending" => return Ok(DevicePoll::Pending),
            "slow_down" => return Ok(DevicePoll::SlowDown),
            "expired_token" | "access_denied" => {
                let desc = parsed.error_description.as_deref().unwrap_or(error);
                return Err(format!("授权失败: {}", desc));
            }
            _ => {
                let desc = parsed.error_description.as_deref().unwrap_or(error);
                return Err(format!("授权失败: {}", desc));
            }
        }
    }

    let token = parsed
        .access_token
        .or(parsed.access_token_alt)
        .ok_or_else(|| "未能获取 access token".to_string())?;

    let mut value = serde_json::json!({ "access_token": token });
    if let Some(tt) = parsed.token_type {
        value["token_type"] = serde_json::Value::String(tt);
    }
    if let Some(scope) = parsed.scope {
        value["scope"] = serde_json::Value::String(scope);
    }
    for (k, v) in parsed.extra {
        if k != "access_token" && k != "error" && k != "error_description" {
            value[k] = v;
        }
    }

    Ok(DevicePoll::Token(value))
}

// ---------------------------------------------------------------------------
// JWT helpers (decode payload without signature verification)
// ---------------------------------------------------------------------------

fn decode_jwt_payload(jwt: &str) -> Option<serde_json::Value> {
    let parts: Vec<&str> = jwt.split('.').collect();
    if parts.len() < 2 {
        return None;
    }
    let payload = parts[1];
    let padded = match payload.len() % 4 {
        2 => format!("{}==", payload),
        3 => format!("{}=", payload),
        _ => payload.to_string(),
    };
    let bytes = base64_decode_urlsafe(&padded)?;
    serde_json::from_slice(&bytes).ok()
}

fn base64_decode_urlsafe(input: &str) -> Option<Vec<u8>> {
    let standard: String = input
        .chars()
        .map(|c| match c {
            '-' => '+',
            '_' => '/',
            other => other,
        })
        .collect();
    base64_decode_standard(&standard)
}

fn base64_decode_standard(input: &str) -> Option<Vec<u8>> {
    let table: Vec<u8> = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/"
        .to_vec();
    let mut buf = Vec::new();
    let mut bits: u32 = 0;
    let mut n_bits: u32 = 0;
    for c in input.bytes() {
        if c == b'=' {
            break;
        }
        let val = table.iter().position(|&b| b == c)? as u32;
        bits = (bits << 6) | val;
        n_bits += 6;
        if n_bits >= 8 {
            n_bits -= 8;
            buf.push((bits >> n_bits) as u8);
            bits &= (1 << n_bits) - 1;
        }
    }
    Some(buf)
}

fn extract_email_from_jwt(token: &serde_json::Value) -> Option<String> {
    if let Some(id_token) = token.get("id_token").and_then(|v| v.as_str()) {
        if let Some(payload) = decode_jwt_payload(id_token) {
            if let Some(email) = payload.get("email").and_then(|v| v.as_str()) {
                return Some(email.to_string());
            }
        }
    }
    if let Some(at) = token.get("access_token").and_then(|v| v.as_str()) {
        if let Some(payload) = decode_jwt_payload(at) {
            if let Some(profile) = payload.get("https://api.openai.com/profile") {
                if let Some(email) = profile.get("email").and_then(|v| v.as_str()) {
                    return Some(email.to_string());
                }
            }
        }
    }
    None
}

fn extract_plan_type_from_jwt(token: &serde_json::Value) -> Option<String> {
    for field in ["id_token", "access_token"] {
        if let Some(jwt) = token.get(field).and_then(|v| v.as_str()) {
            if let Some(payload) = decode_jwt_payload(jwt) {
                let auth = payload
                    .get("https://api.openai.com/auth")
                    .or_else(|| payload.get("https://api.openai.com/auth"));
                if let Some(auth) = auth {
                    if let Some(plan) = auth.get("chatgpt_plan_type").and_then(|v| v.as_str()) {
                        return Some(plan.to_string());
                    }
                }
            }
        }
    }
    None
}

fn extract_account_id_from_jwt(token: &serde_json::Value) -> Option<String> {
    for field in ["id_token", "access_token"] {
        if let Some(jwt) = token.get(field).and_then(|v| v.as_str()) {
            if let Some(payload) = decode_jwt_payload(jwt) {
                let auth = payload.get("https://api.openai.com/auth");
                if let Some(auth) = auth {
                    if let Some(id) = auth.get("chatgpt_account_id").and_then(|v| v.as_str()) {
                        return Some(id.to_string());
                    }
                }
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Write auth file
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Provider-specific user info fetching (for opaque tokens)
// ---------------------------------------------------------------------------

fn fetch_github_user_info(access_token: &str) -> Option<(String, Option<String>)> {
    let resp = http_agent()
        .get("https://api.github.com/user")
        .set("Authorization", &format!("token {}", access_token))
        .set("Accept", "application/json")
        .set("User-Agent", "Quotio-Desktop/0.4")
        .call()
        .ok()?;
    let user: serde_json::Value = resp.into_json().ok()?;

    let login = user.get("login").and_then(|v| v.as_str());
    let email = user
        .get("email")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty());

    if let Some(email) = email {
        return Some((email.to_string(), login.map(|s| s.to_string())));
    }
    // email is private — try /user/emails
    if let Ok(resp2) = http_agent()
        .get("https://api.github.com/user/emails")
        .set("Authorization", &format!("token {}", access_token))
        .set("Accept", "application/json")
        .set("User-Agent", "Quotio-Desktop/0.4")
        .call()
    {
        if let Ok(emails) = resp2.into_json::<Vec<serde_json::Value>>() {
            let primary = emails
                .iter()
                .find(|e| e.get("primary").and_then(|v| v.as_bool()).unwrap_or(false))
                .or_else(|| emails.first());
            if let Some(em) = primary.and_then(|e| e.get("email")).and_then(|v| v.as_str()) {
                return Some((em.to_string(), login.map(|s| s.to_string())));
            }
        }
    }
    // Last resort: use login as identifier.
    login.map(|l| (format!("{}@github", l), Some(l.to_string())))
}

fn fetch_qwen_user_info(access_token: &str) -> Option<String> {
    let resp = http_agent()
        .get("https://openapi.qoder.sh/api/v1/user/info")
        .set("Authorization", &format!("Bearer {}", access_token))
        .set("Accept", "application/json")
        .call()
        .ok()?;
    let user: serde_json::Value = resp.into_json().ok()?;
    user.get("email")
        .or_else(|| user.get("data").and_then(|d| d.get("email")))
        .or_else(|| user.get("username"))
        .or_else(|| user.get("data").and_then(|d| d.get("username")))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

fn fetch_user_info_for_provider(
    provider_id: &str,
    obj: &mut serde_json::Map<String, serde_json::Value>,
) -> Option<String> {
    let access_token = obj
        .get("access_token")
        .and_then(|v| v.as_str())?
        .to_string();

    match provider_id {
        "github-copilot" => {
            let (email, login) = fetch_github_user_info(&access_token)?;
            obj.insert(
                "email".to_string(),
                serde_json::Value::String(email.clone()),
            );
            if let Some(l) = login {
                obj.entry("login")
                    .or_insert_with(|| serde_json::Value::String(l));
            }
            Some(email)
        }
        "qwen" => {
            let email = fetch_qwen_user_info(&access_token)?;
            obj.insert(
                "email".to_string(),
                serde_json::Value::String(email.clone()),
            );
            Some(email)
        }
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Write auth file
// ---------------------------------------------------------------------------

fn write_auth_file(
    provider_id: &str,
    token: &serde_json::Value,
) -> Result<(std::path::PathBuf, Option<String>), String> {
    let dir = quotio_platform::proxy_auth_dir();
    log_oauth_event(
        provider_id,
        None,
        "auth_file_write_start",
        &format!("dir={}", dir.display()),
    );
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建 auth 目录失败: {}", e))?;

    let mut output = token.clone();
    let obj = output
        .as_object_mut()
        .ok_or_else(|| "token 不是 JSON 对象".to_string())?;

    // 1) Try top-level email field.
    let mut email: Option<String> = obj
        .get("email")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());

    // 2) Try decoding JWT id_token / access_token.
    if email.is_none() {
        email = extract_email_from_jwt(token);
    }

    // 3) For opaque-token providers, call their user info API.
    if email.is_none() {
        email = fetch_user_info_for_provider(provider_id, obj);
    }

    if let Some(ref e) = email {
        obj.entry("email")
            .or_insert_with(|| serde_json::Value::String(e.clone()));
    }

    // Set provider type for the proxy to recognize.
    let type_value = match provider_id {
        "codex" | "claude" => provider_id,
        "gemini-cli" => "gemini",
        "github-copilot" => "copilot",
        _ => provider_id,
    };
    obj.entry("type")
        .or_insert_with(|| serde_json::Value::String(type_value.to_string()));

    // Extract account_id from JWT for Codex.
    if provider_id == "codex" || provider_id == "claude" {
        if let Some(account_id) = extract_account_id_from_jwt(token) {
            obj.entry("account_id")
                .or_insert_with(|| serde_json::Value::String(account_id));
        }
    }

    let email_str = email.as_deref().unwrap_or("unknown");
    let sanitized_email = email_str
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '@' || c == '.' || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect::<String>();

    // Build filename: provider-email-plan.json or provider-email.json
    let plan = extract_plan_type_from_jwt(token);
    let filename = if let Some(ref p) = plan {
        format!("{}-{}-{}.json", provider_id, sanitized_email, p)
    } else {
        format!("{}-{}.json", provider_id, sanitized_email)
    };
    let path = dir.join(&filename);

    let content = serde_json::to_string_pretty(&output)
        .map_err(|e| format!("序列化 token 失败: {}", e))?;
    std::fs::write(&path, &content).map_err(|e| format!("写入 auth 文件失败: {}", e))?;
    // 含 access/refresh/id_token 的长期凭据:Unix 上收紧到 0600(Windows no-op)。
    let _ = quotio_platform::set_sensitive_permissions(&path);
    log_oauth_event(
        provider_id,
        None,
        "auth_file_write_success",
        &format!("path={} email={}", path.display(), email.as_deref().unwrap_or("unknown")),
    );

    Ok((path, email))
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

pub fn start_oauth(provider_id: &str) -> Result<OAuthStartResponse, String> {
    let config = provider_config(provider_id)
        .ok_or_else(|| format!("Unsupported provider: {}", provider_id))?;

    // Cancel any existing login.
    cancel_oauth(None).ok();

    match config {
        ProviderOAuthConfig::AuthorizationCode(cfg) => start_auth_code_flow(provider_id, &cfg),
        ProviderOAuthConfig::DeviceCode(cfg) => start_device_code_flow(provider_id, &cfg),
    }
}

fn start_auth_code_flow(
    provider_id: &str,
    cfg: &AuthCodeConfig,
) -> Result<OAuthStartResponse, String> {
    let port = find_available_port(cfg.fixed_port)?;
    let redirect_uri = format!("http://localhost:{}{}", port, cfg.callback_path);
    let state_token = generate_token();
    let code_verifier = generate_token();
    let code_challenge = generate_code_challenge(&code_verifier);

    // Build auth URL.
    let mut params: Vec<(&str, String)> = vec![
        ("redirect_uri", redirect_uri.clone()),
        ("state", state_token.clone()),
    ];

    if !cfg.client_id.is_empty() {
        params.push(("client_id", cfg.client_id.to_string()));
    }
    if !cfg.scopes.is_empty() {
        params.push(("scope", cfg.scopes.to_string()));
    }
    if cfg.use_pkce {
        params.push(("code_challenge", code_challenge.clone()));
        params.push(("code_challenge_method", "S256".to_string()));
    }
    // Standard OAuth params.
    if !cfg.auth_endpoint.contains("kiro") {
        params.push(("response_type", "code".to_string()));
    }
    for (k, v) in cfg.extra_auth_params {
        params.push((k, v.to_string()));
    }

    let auth_url = if cfg.auth_endpoint.contains('?') {
        let sep = "&";
        let qs = params
            .iter()
            .map(|(k, v)| format!("{}={}", urlencoding::encode(k), urlencoding::encode(v)))
            .collect::<Vec<_>>()
            .join(sep);
        format!("{}{}{}", cfg.auth_endpoint, sep, qs)
    } else {
        let qs = params
            .iter()
            .map(|(k, v)| format!("{}={}", urlencoding::encode(k), urlencoding::encode(v)))
            .collect::<Vec<_>>()
            .join("&");
        format!("{}?{}", cfg.auth_endpoint, qs)
    };

    let login_id = generate_token();

    let pending = PendingOAuth {
        login_id: login_id.clone(),
        provider_id: provider_id.to_string(),
        flow: OAuthFlowKind::AuthorizationCode,
        code_verifier: if cfg.use_pkce {
            Some(code_verifier)
        } else {
            None
        },
        redirect_uri: Some(redirect_uri),
        state_token: Some(state_token.clone()),
        callback_port: Some(port),
        auth_url: auth_url.clone(),
        code: None,
        device_code: None,
        user_code: None,
        verification_uri: None,
        interval_seconds: 1,
        next_poll_at: 0,
        expires_at: now_ts() + OAUTH_TIMEOUT_SECONDS,
        error: None,
        completed: false,
        client_id: cfg.client_id.to_string(),
        client_secret: cfg.client_secret.map(|s| s.to_string()),
        token_endpoint: cfg.token_endpoint.to_string(),
        use_pkce: cfg.use_pkce,
    };

    *get_state().lock().map_err(|_| "OAuth 状态锁不可用")? = Some(pending);

    start_callback_server(
        port,
        state_token,
        login_id.clone(),
        cfg.callback_path.to_string(),
    );

    Ok(OAuthStartResponse {
        login_id,
        auth_url,
        user_code: String::new(),
        verification_uri: String::new(),
        provider_id: provider_id.to_string(),
        flow: OAuthFlowKind::AuthorizationCode,
    })
}

fn start_device_code_flow(
    provider_id: &str,
    cfg: &DeviceCodeConfig,
) -> Result<OAuthStartResponse, String> {
    let dc = request_device_code(cfg)?;

    let device_code = dc
        .device_code()
        .ok_or_else(|| "未能获取 device_code".to_string())?
        .to_string();
    let user_code = dc.user_code().unwrap_or("").to_string();
    let verification_uri = dc.verification_uri().unwrap_or("").to_string();
    let login_id = generate_token();

    let auth_url = dc
        .verification_uri_complete
        .as_deref()
        .or(dc.verification_uri_complete_alt.as_deref())
        .unwrap_or(&verification_uri)
        .to_string();

    let pending = PendingOAuth {
        login_id: login_id.clone(),
        provider_id: provider_id.to_string(),
        flow: OAuthFlowKind::DeviceCode,
        code_verifier: None,
        redirect_uri: None,
        state_token: None,
        callback_port: None,
        auth_url: auth_url.clone(),
        code: None,
        device_code: Some(device_code),
        user_code: Some(user_code.clone()),
        verification_uri: Some(verification_uri.clone()),
        interval_seconds: dc.interval.unwrap_or(5).max(1) as u64,
        next_poll_at: 0,
        expires_at: now_ts() + dc.expires_in() as i64,
        error: None,
        completed: false,
        client_id: cfg.client_id.to_string(),
        client_secret: None,
        token_endpoint: cfg.token_endpoint.to_string(),
        use_pkce: false,
    };

    *get_state().lock().map_err(|_| "OAuth 状态锁不可用")? = Some(pending);

    Ok(OAuthStartResponse {
        login_id,
        auth_url,
        user_code,
        verification_uri,
        provider_id: provider_id.to_string(),
        flow: OAuthFlowKind::DeviceCode,
    })
}

/// Poll / complete an ongoing OAuth login.
///
/// For auth-code flows: checks if the callback server received the code, then
/// exchanges it for a token. For device-code flows: polls the token endpoint.
///
/// Returns `status: "pending"` while waiting, `status: "success"` on completion.
pub fn complete_oauth(login_id: &str) -> Result<OAuthCompleteResponse, String> {
    let snapshot = {
        let guard = get_state().lock().map_err(|_| "OAuth 状态锁不可用")?;
        guard.clone().ok_or_else(|| "没有进行中的登录会话".to_string())?
    };

    if snapshot.login_id != login_id {
        return Err("登录会话不匹配".to_string());
    }

    if snapshot.completed {
        return Ok(OAuthCompleteResponse {
            status: "success".to_string(),
            error: None,
            provider_id: snapshot.provider_id,
            account_email: None,
        });
    }

    if let Some(err) = &snapshot.error {
        return Ok(OAuthCompleteResponse {
            status: "error".to_string(),
            error: Some(err.clone()),
            provider_id: snapshot.provider_id,
            account_email: None,
        });
    }

    if snapshot.expires_at <= now_ts() {
        cancel_oauth(Some(login_id)).ok();
        return Ok(OAuthCompleteResponse {
            status: "error".to_string(),
            error: Some("OAuth 授权超时，请重试。".to_string()),
            provider_id: snapshot.provider_id,
            account_email: None,
        });
    }

    match snapshot.flow {
        OAuthFlowKind::AuthorizationCode => complete_auth_code(&snapshot),
        OAuthFlowKind::DeviceCode => complete_device_code(&snapshot),
    }
}

fn complete_auth_code(snapshot: &PendingOAuth) -> Result<OAuthCompleteResponse, String> {
    if snapshot.code.is_none() {
        return Ok(OAuthCompleteResponse {
            status: "pending".to_string(),
            error: None,
            provider_id: snapshot.provider_id.clone(),
            account_email: None,
        });
    }

    let token = match exchange_auth_code(snapshot) {
        Ok(token) => token,
        // 瞬时故障(5xx / 连接断开 / 读超时):不置永久 error,返回 pending 让前端在
        // 有效期内继续轮询重试(code 未被消费)。超时由 complete_oauth 的 expires_at 兜底。
        Err(TokenExchangeError::Retryable(message)) => {
            log_oauth_event(
                &snapshot.provider_id,
                Some(&snapshot.login_id),
                "token_exchange_retry",
                &message,
            );
            return Ok(OAuthCompleteResponse {
                status: "pending".to_string(),
                error: None,
                provider_id: snapshot.provider_id.clone(),
                account_email: None,
            });
        }
        Err(TokenExchangeError::Permanent(error)) => {
            return Ok(complete_error_response(
                &snapshot.login_id,
                &snapshot.provider_id,
                "token_exchange_failed",
                error,
            ));
        }
    };
    let (_path, email) = match write_auth_file(&snapshot.provider_id, &token) {
        Ok(result) => result,
        Err(error) => {
            return Ok(complete_error_response(
                &snapshot.login_id,
                &snapshot.provider_id,
                "auth_file_write_failed",
                error,
            ));
        }
    };

    if let Ok(mut guard) = get_state().lock() {
        if let Some(s) = guard.as_mut() {
            if s.login_id == snapshot.login_id {
                s.completed = true;
            }
        }
    }
    log_oauth_event(
        &snapshot.provider_id,
        Some(&snapshot.login_id),
        "complete_success",
        &format!("account_email={}", email.as_deref().unwrap_or("unknown")),
    );

    Ok(OAuthCompleteResponse {
        status: "success".to_string(),
        error: None,
        provider_id: snapshot.provider_id.clone(),
        account_email: email,
    })
}

fn complete_device_code(snapshot: &PendingOAuth) -> Result<OAuthCompleteResponse, String> {
    let pending_response = || OAuthCompleteResponse {
        status: "pending".to_string(),
        error: None,
        provider_id: snapshot.provider_id.clone(),
        account_email: None,
    };

    // 节流:遵守服务器要求的轮询间隔(及 slow_down 退避)。前端固定 ~2s 调一次,
    // 未到 next_poll_at 就直接返回 pending、不打服务器,避免被判 slow_down/拒绝。
    let now = now_ts();
    if let Ok(guard) = get_state().lock() {
        if let Some(live) = guard.as_ref() {
            if live.login_id == snapshot.login_id && now < live.next_poll_at {
                return Ok(pending_response());
            }
        }
    }

    let outcome = match poll_device_token(snapshot) {
        Ok(outcome) => outcome,
        Err(error) => {
            return Ok(complete_error_response(
                &snapshot.login_id,
                &snapshot.provider_id,
                "device_token_poll_failed",
                error,
            ));
        }
    };

    let token = match outcome {
        DevicePoll::Token(token) => token,
        other => {
            // 仍在等待授权:更新下次可轮询时间;slow_down 时按 RFC 8628 把间隔 +5s。
            if let Ok(mut guard) = get_state().lock() {
                if let Some(live) = guard.as_mut() {
                    if live.login_id == snapshot.login_id {
                        if matches!(other, DevicePoll::SlowDown) {
                            live.interval_seconds = live.interval_seconds.saturating_add(5);
                        }
                        live.next_poll_at = now + live.interval_seconds as i64;
                    }
                }
            }
            return Ok(pending_response());
        }
    };

    let (_path, email) = match write_auth_file(&snapshot.provider_id, &token) {
        Ok(result) => result,
        Err(error) => {
            return Ok(complete_error_response(
                &snapshot.login_id,
                &snapshot.provider_id,
                "auth_file_write_failed",
                error,
            ));
        }
    };

    if let Ok(mut guard) = get_state().lock() {
        if let Some(s) = guard.as_mut() {
            if s.login_id == snapshot.login_id {
                s.completed = true;
            }
        }
    }
    log_oauth_event(
        &snapshot.provider_id,
        Some(&snapshot.login_id),
        "complete_success",
        &format!("account_email={}", email.as_deref().unwrap_or("unknown")),
    );

    Ok(OAuthCompleteResponse {
        status: "success".to_string(),
        error: None,
        provider_id: snapshot.provider_id.clone(),
        account_email: email,
    })
}

pub fn cancel_oauth(login_id: Option<&str>) -> Result<(), String> {
    let mut guard = get_state().lock().map_err(|_| "OAuth 状态锁不可用")?;
    if let Some(state) = guard.as_ref() {
        if let Some(expected) = login_id {
            if state.login_id != expected {
                return Err("登录会话不匹配".to_string());
            }
        }
    }
    *guard = None;
    Ok(())
}

/// Manually submit a callback URL (for when the local callback server can't
/// receive the redirect, e.g. firewall issues).
pub fn submit_callback_url(login_id: &str, callback_url: &str) -> Result<(), String> {
    let mut guard = get_state().lock().map_err(|_| "OAuth 状态锁不可用")?;
    let state = guard
        .as_mut()
        .ok_or_else(|| "没有进行中的登录会话".to_string())?;
    if state.login_id != login_id {
        return Err("登录会话不匹配".to_string());
    }
    if state.flow != OAuthFlowKind::AuthorizationCode {
        return Err("当前登录流程不支持手动提交回调".to_string());
    }

    let expected_state = state
        .state_token
        .as_deref()
        .ok_or_else(|| "缺少 state token".to_string())?;

    let parsed = url::Url::parse(callback_url.trim())
        .map_err(|e| format!("回调链接格式无效: {}", e))?;

    let params: HashMap<String, String> = parsed.query_pairs().map(|(k, v)| (k.to_string(), v.to_string())).collect();

    let cb_state = params.get("state").cloned().unwrap_or_default();
    if cb_state != expected_state {
        return Err("OAuth state 校验失败，请确认链接正确。".to_string());
    }

    let code = params
        .get("code")
        .cloned()
        .filter(|c| !c.is_empty())
        .ok_or_else(|| "回调链接缺少授权 code。".to_string())?;

    state.code = Some(code);
    Ok(())
}

/// Import a raw token/JSON for a provider (paste-based import).
pub fn import_auth_token(provider_id: &str, content: &str) -> Result<(), String> {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return Err("内容不能为空".to_string());
    }

    let token: serde_json::Value = if trimmed.starts_with('{') {
        serde_json::from_str(trimmed).map_err(|e| format!("JSON 格式无效: {}", e))?
    } else {
        serde_json::json!({ "access_token": trimmed })
    };

    write_auth_file(provider_id, &token).map(|_| ())
}

/// Check which providers support native OAuth.
pub fn supports_native_oauth(provider_id: &str) -> bool {
    provider_config(provider_id).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn auth_code_pending_with_token_endpoint(token_endpoint: &str) -> PendingOAuth {
        PendingOAuth {
            login_id: "test-login".to_string(),
            provider_id: "codex".to_string(),
            flow: OAuthFlowKind::AuthorizationCode,
            code_verifier: Some("test-verifier".to_string()),
            redirect_uri: Some("http://localhost:1455/auth/callback".to_string()),
            state_token: Some("test-state".to_string()),
            callback_port: Some(1455),
            auth_url: "https://auth.openai.com/oauth/authorize".to_string(),
            code: Some("test-code".to_string()),
            device_code: None,
            user_code: None,
            verification_uri: None,
            interval_seconds: 2,
            next_poll_at: 0,
            expires_at: now_ts() + 60,
            error: None,
            completed: false,
            client_id: "test-client".to_string(),
            client_secret: None,
            token_endpoint: token_endpoint.to_string(),
            use_pkce: true,
        }
    }

    #[test]
    fn complete_auth_code_returns_pending_on_transient_transport_failure() {
        // 连接被拒/断开属瞬时故障:授权 code 未被消费,应返回 pending 让前端在有效期内
        // 继续轮询重试,而不是把会话钉死为永久 error(对齐 5xx/wsarecv 容错)。
        let pending = auth_code_pending_with_token_endpoint("http://127.0.0.1:9/oauth/token");

        let response = complete_auth_code(&pending)
            .expect("transient failure should not hard-error");

        assert_eq!(response.status, "pending");
        assert_eq!(response.provider_id, "codex");
        assert!(
            response.error.is_none(),
            "transient transport failure must not set a permanent error: {:?}",
            response.error
        );
    }
}
