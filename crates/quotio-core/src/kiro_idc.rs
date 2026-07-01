//! Kiro 组织(AWS IAM Identity Center / awsidc)+ 个人(Builder ID)设备流登录。
//!
//! 走标准 AWS SSO OIDC 设备授权三步:
//!   1. RegisterClient          → 动态拿 clientId / clientSecret
//!   2. StartDeviceAuthorization → 拿 userCode + 验证链接 + deviceCode
//!   3. 轮询 CreateToken         → 用户在浏览器批准后拿 accessToken / refreshToken
//!
//! 产出 `authMethod:"idc"` 凭据(refreshToken + clientId + clientSecret + region),
//! 正是 [`crate::kiro_sidecar`] 里 kiro-rs IdC 刷新(`oidc.{region}.amazonaws.com/token`)
//! 需要的形状。落成 `kiro-idc-<slug>.json` 后由 sidecar 的 `collect_credentials` 收走。
//!
//! 组织版与 Builder ID 版**唯一区别是 `startUrl`**:Builder ID 用固定门户
//! `https://view.awsapps.com/start`;组织版用用户 IAM Identity Center 的 Start URL。
//!
//! 参考实现:开源 proxycast(`src-tauri/.../provider_pool_cmd.rs` 的 Kiro Builder ID 流)——
//! 端点 / scopes / 请求体 / 凭据形状均照它;差别只在把固定 Start URL 换成组织可填。
//!
//! ⚠️ **未取 profileArn**:Builder ID 不需要;组织(Q Developer Pro)账号若因缺 profileArn
//! 报错,再补 `ListAvailableProfiles`(kiro-rs 缺 profileArn 时会跳过注入、不会崩)。
//! Quotio 的 kiro 凭据一贯只带 refreshToken、由 kiro-rs 启动时刷新,所以这里不落 accessToken。

use serde::Serialize;
use serde_json::{json, Value};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::native_oauth::http_agent;

/// AWS Builder ID(个人免费)的固定登录门户。
const BUILDER_ID_START_URL: &str = "https://view.awsapps.com/start";
/// 设备码授权 grant type。
const DEVICE_GRANT: &str = "urn:ietf:params:oauth:grant-type:device_code";
/// Kiro / CodeWhisperer 注册客户端时申请的 scopes(照 proxycast)。
const SCOPES: &[&str] = &[
    "codewhisperer:completions",
    "codewhisperer:analysis",
    "codewhisperer:conversations",
    "codewhisperer:transformations",
    "codewhisperer:taskassist",
];
/// 未指定区域时的默认(Builder ID 用 us-east-1)。
const DEFAULT_REGION: &str = "us-east-1";

fn now_ts() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// 进行中的设备流会话(单例,和 native_oauth 一样一次只跑一个登录)。
#[derive(Clone)]
struct PendingIdc {
    client_id: String,
    client_secret: String,
    device_code: String,
    region: String,
    start_url: String,
    login_option: String,
    interval: u64,
    expires_at: i64,
}

static PENDING: OnceLock<Mutex<Option<PendingIdc>>> = OnceLock::new();
fn pending() -> &'static Mutex<Option<PendingIdc>> {
    PENDING.get_or_init(|| Mutex::new(None))
}

fn clear() {
    if let Ok(mut guard) = pending().lock() {
        *guard = None;
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KiroIdcStartResponse {
    /// 展示给用户去核对的短码。
    pub user_code: String,
    /// 让用户打开的验证链接(优先带完整参数的 complete 版)。
    pub verification_uri: String,
    /// 设备码有效秒数。
    pub expires_in: i64,
    /// 建议轮询间隔(秒)。
    pub interval: u64,
    /// 归一后的登录方式:"awsidc" | "builderid"。
    pub login_option: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KiroIdcPollResponse {
    /// "pending" | "success" | "error"
    pub status: String,
    /// status=="error" 时的原因。
    pub error: Option<String>,
}

fn oidc_base(region: &str) -> String {
    format!("https://oidc.{}.amazonaws.com", region)
}

/// POST JSON。成功返回解析后的 `Value`;失败返回 `(HTTP 状态码?, 响应体/错误串)` 供上层判定
/// 是「继续轮询」还是「停在错误」。传输层错误状态码为 `None`。
fn post_json(url: &str, body: &Value) -> Result<Value, (Option<u16>, String)> {
    match http_agent()
        .post(url)
        .set("Content-Type", "application/json")
        .send_string(&body.to_string())
    {
        Ok(resp) => {
            let text = resp
                .into_string()
                .map_err(|e| (None, format!("读取响应失败: {}", e)))?;
            serde_json::from_str(&text).map_err(|e| (None, format!("解析响应失败: {}", e)))
        }
        Err(ureq::Error::Status(code, resp)) => {
            let text = resp.into_string().unwrap_or_default();
            Err((Some(code), text))
        }
        Err(e) => Err((None, format!("{}", e))),
    }
}

/// 归一登录方式:组织相关的各种写法 → "awsidc";其余(个人 / builder id)→ "builderid"。
fn normalize_login_option(raw: &str) -> String {
    match raw.trim().to_ascii_lowercase().as_str() {
        "awsidc" | "aws-idc" | "idc" | "sso" | "org" | "organization" | "enterprise" | "iam"
        | "iam-identity-center" => "awsidc".to_string(),
        _ => "builderid".to_string(),
    }
}

/// startUrl → 文件名安全 slug(去 scheme、非字母数字压成单个 `-`、小写、截断)。
/// 同一 Start URL 得到同一 slug ⇒ 同一组织/Builder ID 重登覆盖同一份凭据文件,不堆垃圾。
fn slugify(url: &str) -> String {
    let stripped = url
        .trim()
        .trim_start_matches("https://")
        .trim_start_matches("http://");
    let mut out = String::new();
    let mut prev_dash = false;
    for c in stripped.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
            prev_dash = false;
        } else if !prev_dash {
            out.push('-');
            prev_dash = true;
        }
    }
    let s: String = out.trim_matches('-').chars().take(48).collect();
    if s.is_empty() {
        "org".to_string()
    } else {
        s
    }
}

/// 启动设备流:RegisterClient + StartDeviceAuthorization,存下会话,返回 userCode + 验证链接。
///
/// - `login_option`:"awsidc"(组织)或 "builderid"(个人)。
/// - `start_url`:组织版必填(用户 IAM Identity Center 的 Start URL);Builder ID 忽略、用固定门户。
/// - `region`:组织版用 IdC 所在区域;缺省 us-east-1。
pub fn start_login(
    login_option: &str,
    start_url: Option<&str>,
    region: Option<&str>,
) -> Result<KiroIdcStartResponse, String> {
    let login_option = normalize_login_option(login_option);
    let region = region
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(DEFAULT_REGION)
        .to_string();

    let start_url = if login_option == "awsidc" {
        start_url
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| "组织登录需要提供 IAM Identity Center 的 Start URL".to_string())?
            .to_string()
    } else {
        BUILDER_ID_START_URL.to_string()
    };

    // Step 1: 注册 OIDC 公共客户端(每次登录新注册一份,拿一次性的 clientId/clientSecret)。
    let reg_body = json!({
        "clientName": "Quotio Kiro Manager",
        "clientType": "public",
        "scopes": SCOPES,
        "grantTypes": [DEVICE_GRANT, "refresh_token"],
        "issuerUrl": start_url,
    });
    let reg = post_json(&format!("{}/client/register", oidc_base(&region)), &reg_body).map_err(
        |(code, body)| match code {
            Some(c) => format!("注册 OIDC 客户端失败({}): {}", c, body),
            None => format!("注册 OIDC 客户端失败: {}", body),
        },
    )?;
    let client_id = reg["clientId"].as_str().unwrap_or_default().to_string();
    let client_secret = reg["clientSecret"].as_str().unwrap_or_default().to_string();
    if client_id.is_empty() || client_secret.is_empty() {
        return Err("注册响应缺少 clientId / clientSecret".to_string());
    }

    // Step 2: 发起设备授权,拿 userCode + 验证链接 + deviceCode。
    let auth_body = json!({
        "clientId": client_id,
        "clientSecret": client_secret,
        "startUrl": start_url,
    });
    let auth = post_json(
        &format!("{}/device_authorization", oidc_base(&region)),
        &auth_body,
    )
    .map_err(|(code, body)| match code {
        Some(c) => format!("发起设备授权失败({}): {}", c, body),
        None => format!("发起设备授权失败: {}", body),
    })?;

    let device_code = auth["deviceCode"].as_str().unwrap_or_default().to_string();
    let user_code = auth["userCode"].as_str().unwrap_or_default().to_string();
    let verification_uri = auth["verificationUriComplete"]
        .as_str()
        .or_else(|| auth["verificationUri"].as_str())
        .unwrap_or_default()
        .to_string();
    let interval = auth["interval"].as_u64().unwrap_or(5).max(1);
    let expires_in = auth["expiresIn"].as_i64().unwrap_or(600);
    if device_code.is_empty() || user_code.is_empty() || verification_uri.is_empty() {
        return Err("设备授权响应缺少 deviceCode / userCode / verificationUri".to_string());
    }

    {
        let mut guard = pending().lock().map_err(|_| "登录状态锁不可用".to_string())?;
        *guard = Some(PendingIdc {
            client_id,
            client_secret,
            device_code,
            region,
            start_url,
            login_option: login_option.clone(),
            interval,
            expires_at: now_ts() + expires_in,
        });
    }

    Ok(KiroIdcStartResponse {
        user_code,
        verification_uri,
        expires_in,
        interval,
        login_option,
    })
}

/// 轮询一次 CreateToken。
/// - 授权成功 → 落盘 idc 凭据、清状态,返回 `status="success"`。
/// - 用户还没批准 → `"pending"`(前端按 interval 继续轮询)。
/// - 过期 / 拒绝 / 其它错误 → 清状态,返回 `"error"` + 原因。
/// - 传输层瞬时错误 → 不清状态,返回 `"pending"`(带 error 说明,前端可继续)。
pub fn poll_login() -> Result<KiroIdcPollResponse, String> {
    let state = {
        let guard = pending().lock().map_err(|_| "登录状态锁不可用".to_string())?;
        guard.clone()
    };
    let Some(state) = state else {
        return Ok(KiroIdcPollResponse {
            status: "error".into(),
            error: Some("没有进行中的登录".into()),
        });
    };

    if now_ts() > state.expires_at {
        clear();
        return Ok(KiroIdcPollResponse {
            status: "error".into(),
            error: Some("授权已过期,请重新发起登录".into()),
        });
    }

    let token_body = json!({
        "clientId": state.client_id,
        "clientSecret": state.client_secret,
        "grantType": DEVICE_GRANT,
        "deviceCode": state.device_code,
    });

    match post_json(&format!("{}/token", oidc_base(&state.region)), &token_body) {
        Ok(token) => {
            persist_credential(&token, &state)?;
            clear();
            Ok(KiroIdcPollResponse {
                status: "success".into(),
                error: None,
            })
        }
        // AWS SSO OIDC 把「还没批准 / 太快 / 已过期 / 被拒」都用 400 + error 字段表达。
        Err((Some(400), body)) => {
            let err = serde_json::from_str::<Value>(&body)
                .ok()
                .and_then(|v| v["error"].as_str().map(str::to_string))
                .unwrap_or_default();
            match err.as_str() {
                "authorization_pending" => Ok(KiroIdcPollResponse {
                    status: "pending".into(),
                    error: None,
                }),
                "slow_down" => {
                    // 轮询太快:调大间隔,继续等。
                    if let Ok(mut guard) = pending().lock() {
                        if let Some(s) = guard.as_mut() {
                            s.interval = s.interval.saturating_add(5);
                        }
                    }
                    Ok(KiroIdcPollResponse {
                        status: "pending".into(),
                        error: None,
                    })
                }
                "expired_token" => {
                    clear();
                    Ok(KiroIdcPollResponse {
                        status: "error".into(),
                        error: Some("设备码已过期,请重新发起登录".into()),
                    })
                }
                "access_denied" => {
                    clear();
                    Ok(KiroIdcPollResponse {
                        status: "error".into(),
                        error: Some("授权被拒绝".into()),
                    })
                }
                other => {
                    clear();
                    let detail = if other.is_empty() { body } else { other.to_string() };
                    Ok(KiroIdcPollResponse {
                        status: "error".into(),
                        error: Some(format!("授权失败: {}", detail)),
                    })
                }
            }
        }
        Err((Some(code), body)) => {
            clear();
            Ok(KiroIdcPollResponse {
                status: "error".into(),
                error: Some(format!("Token 请求失败({}): {}", code, body)),
            })
        }
        // 传输层(连接/超时)瞬时错误:保留会话,让前端下一拍继续轮询。
        Err((None, msg)) => Ok(KiroIdcPollResponse {
            status: "pending".into(),
            error: Some(msg),
        }),
    }
}

/// 取消进行中的设备流登录。
pub fn cancel_login() {
    clear();
}

/// 把设备流拿到的 token 落成 `kiro-idc-<slug>.json`。只写 kiro-rs IdC 刷新必需的字段
/// (authMethod=idc + refreshToken + clientId + clientSecret + region);不落 accessToken,
/// 与 Quotio 既有「kiro 凭据只带 refreshToken、由 kiro-rs 启动时刷新」的做法一致。
fn persist_credential(token: &Value, state: &PendingIdc) -> Result<(), String> {
    let refresh_token = token["refreshToken"].as_str().unwrap_or_default();
    if refresh_token.is_empty() {
        return Err(
            "Token 响应缺少 refreshToken —— 设备流须拿到 refresh_token 才能续期(检查注册时是否申请了 refresh_token grant)"
                .to_string(),
        );
    }

    let mut cred = serde_json::Map::new();
    cred.insert("type".into(), json!("kiro"));
    cred.insert("authMethod".into(), json!("idc"));
    cred.insert("refreshToken".into(), json!(refresh_token));
    cred.insert("clientId".into(), json!(state.client_id));
    cred.insert("clientSecret".into(), json!(state.client_secret));
    cred.insert("region".into(), json!(state.region));
    // 保留来源信息,便于诊断 / 未来补 profileArn。
    cred.insert("startUrl".into(), json!(state.start_url));
    cred.insert("loginOption".into(), json!(state.login_option));

    let dir = quotio_platform::proxy_auth_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建 auth 目录失败: {}", e))?;
    let path = dir.join(format!("kiro-idc-{}.json", slugify(&state.start_url)));
    let content = serde_json::to_string_pretty(&Value::Object(cred))
        .map_err(|e| format!("序列化凭据失败: {}", e))?;
    std::fs::write(&path, &content).map_err(|e| format!("写入凭据失败: {}", e))?;
    // 含长期 refreshToken/clientSecret:Unix 上收紧到 0600(Windows no-op)。
    let _ = quotio_platform::set_sensitive_permissions(&path);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_org_variants_to_awsidc() {
        for v in [
            "awsidc",
            "AWSIDC",
            "aws-idc",
            "idc",
            "sso",
            "org",
            "organization",
            "enterprise",
            "iam",
            "iam-identity-center",
        ] {
            assert_eq!(normalize_login_option(v), "awsidc", "{v} 应归一到 awsidc");
        }
    }

    #[test]
    fn normalizes_personal_to_builderid() {
        for v in ["builderid", "builder-id", "personal", "", "google", "github"] {
            assert_eq!(normalize_login_option(v), "builderid", "{v} 应归一到 builderid");
        }
    }

    #[test]
    fn slugify_is_filename_safe_and_stable() {
        assert_eq!(
            slugify("https://view.awsapps.com/start"),
            "view-awsapps-com-start"
        );
        // 同一 URL → 同一 slug(重登覆盖同文件)。
        assert_eq!(
            slugify("https://d-1234567890.awsapps.com/start"),
            slugify("https://d-1234567890.awsapps.com/start/")
        );
        // 结果不含路径分隔符等非法文件名字符。
        let s = slugify("https://my.org/sso/start?x=1");
        assert!(!s.contains('/') && !s.contains('?') && !s.contains(':'));
        assert!(!s.is_empty());
    }

    #[test]
    fn slugify_falls_back_when_empty() {
        assert_eq!(slugify("://"), "org");
        assert_eq!(slugify(""), "org");
    }

    #[test]
    fn oidc_base_is_region_scoped() {
        assert_eq!(oidc_base("us-east-1"), "https://oidc.us-east-1.amazonaws.com");
        assert_eq!(oidc_base("eu-west-1"), "https://oidc.eu-west-1.amazonaws.com");
    }
}
