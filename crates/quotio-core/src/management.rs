use std::{
    io::{Read as _, Write as _},
    net::TcpStream,
    time::Duration,
};

use quotio_types::{
    APICallRequest, APICallResponse, ApiKeyUpdateRequest, ApiKeysResponse, AuthFile,
    AuthFileModelInfo, AuthFileModelsResponse, AuthFileStatusRequest, BoolValueRequest,
    DebugResponse, IntegerValueRequest, LatestVersionResponse, LoggingToFileResponse, LogsResponse,
    ManagementSnapshot, MaxRetryIntervalResponse, OAuthStatusResponse, OAuthUrlResponse,
    ProxyUrlResponse, RemoteProxyConfig, RequestLogEntry, RequestLogResponse, RequestRetryResponse,
    RoutingStrategyResponse, StringValueRequest, SwitchPreviewModelResponse, SwitchProjectResponse,
    UsageStats,
};
use serde::{de::DeserializeOwned, Deserialize, Serialize};

#[derive(Debug, Clone)]
pub struct ManagementApiClient {
    base_url: String,
    auth_key: String,
}

#[derive(Debug)]
pub enum ManagementApiError {
    Http(String),
    Status(u16),
    Json(String),
}

impl std::fmt::Display for ManagementApiError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Http(message) => write!(formatter, "管理接口请求失败：{}", message),
            Self::Status(status) => write!(formatter, "管理接口返回 HTTP {}", status),
            Self::Json(message) => write!(formatter, "管理接口响应解析失败：{}", message),
        }
    }
}

impl std::error::Error for ManagementApiError {}

impl ManagementApiClient {
    pub fn local(base_url: impl Into<String>, auth_key: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            auth_key: auth_key.into(),
        }
    }

    pub async fn get_debug(&self) -> Result<bool, ManagementApiError> {
        let response: DebugResponse = self.get_json("/debug")?;
        Ok(response.debug)
    }

    pub async fn get_routing_strategy(&self) -> Result<String, ManagementApiError> {
        match self.get_json::<RoutingStrategyResponse>("/routing/strategy") {
            Ok(response) => Ok(response.strategy),
            Err(ManagementApiError::Status(404)) => {
                let response: RoutingStrategyResponse = self.get_json("/routing")?;
                Ok(response.strategy)
            }
            Err(error) => Err(error),
        }
    }

    pub async fn refresh_snapshot(&self) -> Result<ManagementSnapshot, ManagementApiError> {
        Ok(ManagementSnapshot {
            auth_files: self.fetch_auth_files().await?,
            usage: self.fetch_optional_usage_stats().await?,
            api_keys: self.fetch_api_keys().await?,
            config: Some(self.fetch_config().await?),
            logs: self.fetch_optional_logs(None).await?,
            latest_version: None,
        })
    }

    pub async fn fetch_auth_files(&self) -> Result<Vec<AuthFile>, ManagementApiError> {
        let body = self.request("GET", "/auth-files", None)?;
        decode_auth_files_response(&body)
    }

    pub async fn fetch_usage_stats(&self) -> Result<UsageStats, ManagementApiError> {
        self.get_json("/usage")
    }

    async fn fetch_optional_usage_stats(&self) -> Result<Option<UsageStats>, ManagementApiError> {
        match self.fetch_usage_stats().await {
            Ok(stats) => Ok(Some(stats)),
            Err(ManagementApiError::Status(404)) => Ok(None),
            Err(error) => Err(error),
        }
    }

    pub async fn fetch_api_keys(&self) -> Result<Vec<String>, ManagementApiError> {
        let response: ApiKeysResponse = self.get_json("/api-keys")?;
        Ok(response.api_keys)
    }

    pub async fn fetch_config(&self) -> Result<RemoteProxyConfig, ManagementApiError> {
        self.get_json("/config")
    }

    pub async fn fetch_logs(&self, after: Option<u64>) -> Result<LogsResponse, ManagementApiError> {
        let path = match after {
            Some(after) => format!("/logs?after={}", after),
            None => "/logs".to_string(),
        };
        self.get_json(&path)
    }

    async fn fetch_optional_logs(
        &self,
        after: Option<u64>,
    ) -> Result<Option<LogsResponse>, ManagementApiError> {
        match self.fetch_logs(after).await {
            Ok(logs) => Ok(Some(logs)),
            Err(ManagementApiError::Status(400 | 404)) => Ok(None),
            Err(error) => Err(error),
        }
    }

    pub async fn set_proxy_url(&self, url: impl Into<String>) -> Result<(), ManagementApiError> {
        self.request_json(
            "PUT",
            "/proxy-url",
            &StringValueRequest { value: url.into() },
        )?;
        Ok(())
    }

    pub async fn replace_api_keys(&self, keys: Vec<String>) -> Result<(), ManagementApiError> {
        self.request_json("PUT", "/api-keys", &keys)?;
        Ok(())
    }

    pub async fn fetch_auth_file_models(
        &self,
        name: &str,
    ) -> Result<Vec<AuthFileModelInfo>, ManagementApiError> {
        let encoded = urlencoding::encode(name);
        let response: AuthFileModelsResponse =
            self.get_json(&format!("/auth-files/models?name={}", encoded))?;
        Ok(response.models)
    }

    pub async fn api_call(
        &self,
        request: &APICallRequest,
    ) -> Result<APICallResponse, ManagementApiError> {
        let body = self.request_json("POST", "/api-call", request)?;
        serde_json::from_str(&body).map_err(|error| ManagementApiError::Json(error.to_string()))
    }

    /// Send a tiny "warmup" request through an Antigravity account to keep it
    /// active, mirroring the macOS WarmupService. Tries the known endpoints and
    /// returns the first response status code (2xx means warmed).
    pub async fn warmup_antigravity(
        &self,
        auth_index: &str,
        model: &str,
    ) -> Result<u16, ManagementApiError> {
        let upstream = map_antigravity_model(model);
        let session = uuid::Uuid::new_v4().simple().to_string();
        let body = serde_json::json!({
            "project": format!("warmup-{}", &session[..5]),
            "requestId": format!("agent-{}", uuid::Uuid::new_v4()),
            "userAgent": "antigravity",
            "model": upstream,
            "request": {
                "sessionId": format!("-{}", &session[..12]),
                "contents": [{ "role": "user", "parts": [{ "text": "." }] }],
                "generationConfig": { "maxOutputTokens": 1 }
            }
        })
        .to_string();

        let mut header = std::collections::BTreeMap::new();
        header.insert("Authorization".to_string(), "Bearer $TOKEN$".to_string());
        header.insert("Content-Type".to_string(), "application/json".to_string());
        header.insert("User-Agent".to_string(), "antigravity/1.104.0".to_string());

        const BASES: [&str; 3] = [
            "https://daily-cloudcode-pa.googleapis.com",
            "https://daily-cloudcode-pa.sandbox.googleapis.com",
            "https://cloudcode-pa.googleapis.com",
        ];

        let mut last_status = 0u16;
        for base in BASES {
            let request = APICallRequest {
                auth_index: Some(auth_index.to_string()),
                method: "POST".to_string(),
                url: format!("{}/v1internal:generateContent", base),
                header: Some(header.clone()),
                data: Some(body.clone()),
            };
            if let Ok(response) = self.api_call(&request).await {
                last_status = response.status_code;
                if (200..300).contains(&response.status_code) {
                    return Ok(response.status_code);
                }
            }
        }
        Ok(last_status)
    }

    pub async fn delete_auth_file(&self, name: &str) -> Result<(), ManagementApiError> {
        let encoded = urlencoding::encode(name);
        self.request_empty("DELETE", &format!("/auth-files?name={}", encoded))
    }

    pub async fn delete_all_auth_files(&self) -> Result<(), ManagementApiError> {
        self.request_empty("DELETE", "/auth-files?all=true")
    }

    pub async fn set_auth_file_disabled(
        &self,
        name: impl Into<String>,
        disabled: bool,
    ) -> Result<(), ManagementApiError> {
        self.request_json(
            "PATCH",
            "/auth-files/status",
            &AuthFileStatusRequest {
                name: name.into(),
                disabled,
            },
        )?;
        Ok(())
    }

    pub async fn get_oauth_url(
        &self,
        endpoint: &str,
        project_id: Option<&str>,
        is_webui: bool,
    ) -> Result<OAuthUrlResponse, ManagementApiError> {
        self.get_json(&oauth_endpoint(endpoint, project_id, is_webui))
    }

    pub async fn poll_oauth_status(
        &self,
        state: &str,
    ) -> Result<OAuthStatusResponse, ManagementApiError> {
        let encoded = urlencoding::encode(state);
        self.get_json(&format!("/get-auth-status?state={}", encoded))
    }

    pub async fn clear_logs(&self) -> Result<(), ManagementApiError> {
        self.request_empty("DELETE", "/logs")
    }

    pub async fn set_debug(&self, enabled: bool) -> Result<(), ManagementApiError> {
        self.request_json("PUT", "/debug", &BoolValueRequest { value: enabled })?;
        Ok(())
    }

    pub async fn set_routing_strategy(
        &self,
        strategy: impl Into<String>,
    ) -> Result<(), ManagementApiError> {
        let strategy = strategy.into();
        match self.request_json(
            "PUT",
            "/routing/strategy",
            &StringValueRequest {
                value: strategy.clone(),
            },
        ) {
            Ok(_) => Ok(()),
            Err(ManagementApiError::Status(404)) => {
                self.request_json("PUT", "/routing", &LegacyRoutingRequest { strategy })?;
                Ok(())
            }
            Err(error) => Err(error),
        }
    }

    pub async fn set_quota_exceeded_switch_project(
        &self,
        enabled: bool,
    ) -> Result<(), ManagementApiError> {
        self.request_json(
            "PATCH",
            "/quota-exceeded/switch-project",
            &BoolValueRequest { value: enabled },
        )?;
        Ok(())
    }

    pub async fn set_quota_exceeded_switch_preview_model(
        &self,
        enabled: bool,
    ) -> Result<(), ManagementApiError> {
        self.request_json(
            "PATCH",
            "/quota-exceeded/switch-preview-model",
            &BoolValueRequest { value: enabled },
        )?;
        Ok(())
    }

    pub async fn set_request_retry(&self, count: u16) -> Result<(), ManagementApiError> {
        self.request_json(
            "PUT",
            "/request-retry",
            &IntegerValueRequest { value: count },
        )?;
        Ok(())
    }

    pub async fn get_proxy_url(&self) -> Result<String, ManagementApiError> {
        let response: ProxyUrlResponse = self.get_json("/proxy-url")?;
        Ok(response.proxy_url)
    }

    pub async fn delete_proxy_url(&self) -> Result<(), ManagementApiError> {
        self.request_empty("DELETE", "/proxy-url")
    }

    pub async fn get_logging_to_file(&self) -> Result<bool, ManagementApiError> {
        let response: LoggingToFileResponse = self.get_json("/logging-to-file")?;
        Ok(response.logging_to_file)
    }

    pub async fn set_logging_to_file(&self, enabled: bool) -> Result<(), ManagementApiError> {
        self.request_json(
            "PUT",
            "/logging-to-file",
            &BoolValueRequest { value: enabled },
        )?;
        Ok(())
    }

    pub async fn get_request_log(&self) -> Result<bool, ManagementApiError> {
        let response: RequestLogResponse = self.get_json("/request-log")?;
        Ok(response.request_log)
    }

    pub async fn set_request_log(&self, enabled: bool) -> Result<(), ManagementApiError> {
        self.request_json("PUT", "/request-log", &BoolValueRequest { value: enabled })?;
        Ok(())
    }

    /// Enable/disable per-request usage telemetry (fills the `/usage-queue`).
    pub async fn set_usage_statistics_enabled(&self, enabled: bool) -> Result<(), ManagementApiError> {
        self.request_json(
            "PUT",
            "/usage-statistics-enabled",
            &BoolValueRequest { value: enabled },
        )?;
        Ok(())
    }

    /// Drain up to `count` per-request records from the proxy's usage queue and
    /// map them into structured request-log entries. Records are removed from
    /// the queue on retrieval, so callers should accumulate them.
    pub async fn fetch_request_logs(
        &self,
        count: u32,
    ) -> Result<Vec<RequestLogEntry>, ManagementApiError> {
        let records: Vec<UsageRecord> =
            match self.get_json(&format!("/usage-queue?count={}", count)) {
                Ok(records) => records,
                Err(ManagementApiError::Status(400 | 404)) => return Ok(Vec::new()),
                Err(error) => return Err(error),
            };
        Ok(records.into_iter().map(UsageRecord::into_request_log).collect())
    }

    pub async fn get_request_retry(&self) -> Result<u16, ManagementApiError> {
        let response: RequestRetryResponse = self.get_json("/request-retry")?;
        Ok(response.request_retry)
    }

    pub async fn get_max_retry_interval(&self) -> Result<u16, ManagementApiError> {
        let response: MaxRetryIntervalResponse = self.get_json("/max-retry-interval")?;
        Ok(response.max_retry_interval)
    }

    pub async fn set_max_retry_interval(&self, seconds: u16) -> Result<(), ManagementApiError> {
        self.request_json(
            "PUT",
            "/max-retry-interval",
            &IntegerValueRequest { value: seconds },
        )?;
        Ok(())
    }

    pub async fn get_quota_exceeded_switch_project(&self) -> Result<bool, ManagementApiError> {
        let response: SwitchProjectResponse = self.get_json("/quota-exceeded/switch-project")?;
        Ok(response.switch_project)
    }

    pub async fn get_quota_exceeded_switch_preview_model(
        &self,
    ) -> Result<bool, ManagementApiError> {
        let response: SwitchPreviewModelResponse =
            self.get_json("/quota-exceeded/switch-preview-model")?;
        Ok(response.switch_preview_model)
    }

    pub async fn upload_vertex_service_account(
        &self,
        json: impl AsRef<str>,
    ) -> Result<(), ManagementApiError> {
        self.request("POST", "/vertex/import", Some(json.as_ref()))?;
        Ok(())
    }

    pub async fn add_api_key(&self, key: impl Into<String>) -> Result<(), ManagementApiError> {
        let mut keys = self.fetch_api_keys().await?;
        keys.push(key.into());
        self.replace_api_keys(keys).await
    }

    pub async fn update_api_key(
        &self,
        old: impl Into<String>,
        new: impl Into<String>,
    ) -> Result<(), ManagementApiError> {
        self.request_json(
            "PATCH",
            "/api-keys",
            &ApiKeyUpdateRequest {
                old: old.into(),
                new: new.into(),
            },
        )?;
        Ok(())
    }

    pub async fn delete_api_key(&self, value: &str) -> Result<(), ManagementApiError> {
        let encoded = urlencoding::encode(value);
        self.request_empty("DELETE", &format!("/api-keys?value={}", encoded))
    }

    pub async fn delete_api_key_by_index(&self, index: usize) -> Result<(), ManagementApiError> {
        self.request_empty("DELETE", &format!("/api-keys?index={}", index))
    }

    pub async fn fetch_latest_version(&self) -> Result<LatestVersionResponse, ManagementApiError> {
        self.get_json("/latest-version")
    }

    pub async fn check_proxy_responding(&self) -> bool {
        self.get_debug().await.is_ok()
    }

    fn get_json<T: DeserializeOwned>(&self, path: &str) -> Result<T, ManagementApiError> {
        let body = self.request("GET", path, None)?;
        serde_json::from_str(&body).map_err(|error| ManagementApiError::Json(error.to_string()))
    }

    fn request_json<T: Serialize>(
        &self,
        method: &str,
        path: &str,
        body: &T,
    ) -> Result<String, ManagementApiError> {
        let body = serde_json::to_string(body)
            .map_err(|error| ManagementApiError::Json(error.to_string()))?;
        self.request(method, path, Some(&body))
    }

    fn request_empty(&self, method: &str, path: &str) -> Result<(), ManagementApiError> {
        self.request(method, path, None)?;
        Ok(())
    }

    fn request(
        &self,
        method: &str,
        path: &str,
        body: Option<&str>,
    ) -> Result<String, ManagementApiError> {
        let target = HttpTarget::parse(&self.base_url, path)?;
        let body = body.unwrap_or_default();
        let mut stream = TcpStream::connect((&*target.host, target.port))
            .map_err(|error| ManagementApiError::Http(error.to_string()))?;
        let timeout = Duration::from_secs(15);
        stream
            .set_read_timeout(Some(timeout))
            .map_err(|error| ManagementApiError::Http(error.to_string()))?;
        stream
            .set_write_timeout(Some(timeout))
            .map_err(|error| ManagementApiError::Http(error.to_string()))?;

        let request = format!(
            "{} {} HTTP/1.1\r\nHost: {}\r\nAuthorization: Bearer {}\r\nContent-Type: application/json\r\nAccept: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            method,
            target.path,
            target.host_header(),
            self.auth_key,
            body.len(),
            body,
        );
        stream
            .write_all(request.as_bytes())
            .map_err(|error| ManagementApiError::Http(error.to_string()))?;

        let mut bytes = Vec::new();
        stream
            .read_to_end(&mut bytes)
            .map_err(|error| ManagementApiError::Http(error.to_string()))?;
        parse_http_response(&bytes)
    }
}

fn decode_auth_files_response(body: &str) -> Result<Vec<AuthFile>, ManagementApiError> {
    let value = serde_json::from_str::<serde_json::Value>(body)
        .map_err(|error| ManagementApiError::Json(error.to_string()))?;

    match value {
        serde_json::Value::Object(mut object) => {
            for key in ["files", "auth_files", "authFiles"] {
                if let Some(files) = object.remove(key) {
                    return decode_auth_file_list(files);
                }
            }

            if object
                .keys()
                .any(|key| matches!(key.as_str(), "count" | "total" | "ready"))
            {
                return Ok(Vec::new());
            }

            Err(ManagementApiError::Json(
                "auth-files response did not include a files list".to_string(),
            ))
        }
        serde_json::Value::Array(_) => decode_auth_file_list(value),
        serde_json::Value::Number(_) | serde_json::Value::Null => Ok(Vec::new()),
        _ => Err(ManagementApiError::Json(
            "auth-files response used an unsupported shape".to_string(),
        )),
    }
}

fn decode_auth_file_list(value: serde_json::Value) -> Result<Vec<AuthFile>, ManagementApiError> {
    serde_json::from_value(value).map_err(|error| ManagementApiError::Json(error.to_string()))
}

#[derive(Debug, Deserialize)]
struct UsageRecord {
    #[serde(default)]
    timestamp: Option<serde_json::Value>,
    #[serde(default)]
    latency_ms: Option<u64>,
    #[serde(default)]
    failed: Option<bool>,
    #[serde(default)]
    provider: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    alias: Option<String>,
    #[serde(default)]
    endpoint: Option<String>,
    #[serde(default)]
    request_id: Option<String>,
    #[serde(default)]
    tokens: Option<UsageTokens>,
    #[serde(default)]
    reasoning_effort: Option<String>,
    #[serde(default, alias = "account", alias = "email")]
    source: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct UsageTokens {
    #[serde(default, alias = "input_tokens")]
    input: Option<u64>,
    #[serde(default, alias = "output_tokens")]
    output: Option<u64>,
}

impl UsageRecord {
    fn into_request_log(self) -> RequestLogEntry {
        let failed = self.failed.unwrap_or(false);
        let tokens = self.tokens.unwrap_or_default();
        RequestLogEntry {
            id: self.request_id.unwrap_or_default(),
            timestamp: format_usage_timestamp(self.timestamp),
            method: "POST".to_string(),
            endpoint: self.endpoint.unwrap_or_default(),
            provider: self.provider,
            model: self.model.or(self.alias),
            resolved_model: None,
            resolved_provider: None,
            input_tokens: tokens.input,
            output_tokens: tokens.output,
            duration_ms: self.latency_ms.unwrap_or(0),
            status_code: Some(if failed { 500 } else { 200 }),
            request_size: 0,
            response_size: 0,
            error_message: if failed { Some("请求失败".to_string()) } else { None },
            fallback_attempts: None,
            fallback_started_from_cache: false,
            reasoning_effort: self.reasoning_effort,
            account: self.source.filter(|value| !value.trim().is_empty()),
        }
    }
}

/// Map Antigravity model aliases to upstream model ids for warmup requests.
fn map_antigravity_model(model: &str) -> String {
    match model.to_lowercase().as_str() {
        "gemini-3-pro-preview" => "gemini-3-pro-high",
        "gemini-3-flash-preview" => "gemini-3-flash",
        "gemini-2.5-flash-preview" => "gemini-2.5-flash",
        other => other,
    }
    .to_string()
}

/// Format a unix timestamp (seconds or milliseconds) as an RFC3339 string.
fn format_usage_timestamp(value: Option<serde_json::Value>) -> String {
    match value {
        // Newer CLIProxyAPI emits an RFC3339 string; use it as-is.
        Some(serde_json::Value::String(text)) => text,
        // Older builds emit a unix timestamp (seconds or milliseconds).
        Some(serde_json::Value::Number(number)) => {
            let value = number.as_f64().unwrap_or(0.0);
            let secs = if value > 1.0e12 { (value / 1000.0) as i64 } else { value as i64 };
            chrono::DateTime::from_timestamp(secs, 0)
                .map(|dt| dt.to_rfc3339())
                .unwrap_or_default()
        }
        _ => String::new(),
    }
}

#[cfg(test)]
mod usage_record_parse_test {
    use super::UsageRecord;
    #[test]
    fn parses_real_record() {
        let json = r#"[{"timestamp":"2026-06-07T20:06:46.39+08:00","latency_ms":2312,"source":"x@icloud.com","tokens":{"input_tokens":303,"output_tokens":13,"total_tokens":316},"failed":false,"provider":"codex","model":"gpt-5.4-mini","alias":"gpt-5.4-mini","endpoint":"POST /v1/chat/completions","request_id":"f027c98c"}]"#;
        let records: Vec<UsageRecord> = serde_json::from_str(json).expect("should parse");
        assert_eq!(records.len(), 1);
        let entry = records.into_iter().next().unwrap().into_request_log();
        eprintln!(
            "TS={} PROVIDER={:?} MODEL={:?} IN={:?} OUT={:?} DUR={}",
            entry.timestamp,
            entry.provider,
            entry.model,
            entry.input_tokens,
            entry.output_tokens,
            entry.duration_ms
        );
        assert!(!entry.timestamp.is_empty());
        assert_eq!(entry.input_tokens, Some(303));
        assert_eq!(entry.output_tokens, Some(13));
        assert_eq!(entry.provider.as_deref(), Some("codex"));
        assert_eq!(entry.duration_ms, 2312);
    }
}

struct HttpTarget {
    host: String,
    port: u16,
    path: String,
}

impl HttpTarget {
    fn parse(base_url: &str, endpoint_path: &str) -> Result<Self, ManagementApiError> {
        let Some(raw) = base_url.strip_prefix("http://") else {
            return Err(ManagementApiError::Http(
                "当前管理接口客户端仅支持 http:// 端点".to_string(),
            ));
        };

        let (authority, base_path) = raw.split_once('/').unwrap_or((raw, ""));
        let (host, port) = parse_authority(authority)?;
        let base_path = base_path.trim_matches('/');
        let endpoint_path = endpoint_path.trim_start_matches('/');
        let path = match (base_path.is_empty(), endpoint_path.is_empty()) {
            (true, true) => "/".to_string(),
            (true, false) => format!("/{}", endpoint_path),
            (false, true) => format!("/{}", base_path),
            (false, false) => format!("/{}/{}", base_path, endpoint_path),
        };

        Ok(Self { host, port, path })
    }

    fn host_header(&self) -> String {
        if self.port == 80 {
            self.host.clone()
        } else {
            format!("{}:{}", self.host, self.port)
        }
    }
}

#[derive(Serialize)]
struct LegacyRoutingRequest {
    strategy: String,
}

fn oauth_endpoint(endpoint: &str, project_id: Option<&str>, is_webui: bool) -> String {
    let mut query = Vec::new();
    if let Some(project_id) = project_id.filter(|value| !value.trim().is_empty()) {
        query.push(format!("project_id={}", urlencoding::encode(project_id)));
    }
    if is_webui {
        query.push("is_webui=true".to_string());
    }

    if query.is_empty() {
        endpoint.to_string()
    } else if endpoint.contains('?') {
        format!("{}&{}", endpoint, query.join("&"))
    } else {
        format!("{}?{}", endpoint, query.join("&"))
    }
}

fn parse_authority(authority: &str) -> Result<(String, u16), ManagementApiError> {
    let (host, port) = authority.rsplit_once(':').unwrap_or((authority, "80"));
    let port = port
        .parse::<u16>()
        .map_err(|error| ManagementApiError::Http(format!("无效端口：{}", error)))?;
    if host.trim().is_empty() {
        return Err(ManagementApiError::Http("管理接口主机为空".to_string()));
    }
    Ok((host.to_string(), port))
}

fn parse_http_response(bytes: &[u8]) -> Result<String, ManagementApiError> {
    let raw = String::from_utf8_lossy(bytes);
    let (head, body) = raw
        .split_once("\r\n\r\n")
        .ok_or_else(|| ManagementApiError::Http("管理接口响应格式无效".to_string()))?;
    let status_line = head.lines().next().unwrap_or_default();
    let status = status_line
        .split_whitespace()
        .nth(1)
        .ok_or_else(|| ManagementApiError::Http("管理接口响应缺少状态码".to_string()))?
        .parse::<u16>()
        .map_err(|error| ManagementApiError::Http(format!("无效状态码：{}", error)))?;

    if !(200..=299).contains(&status) {
        return Err(ManagementApiError::Status(status));
    }

    if has_chunked_transfer_encoding(head) {
        return decode_chunked_body(body.as_bytes());
    }

    Ok(body.to_string())
}

fn has_chunked_transfer_encoding(head: &str) -> bool {
    head.lines().skip(1).any(|line| {
        let Some((name, value)) = line.split_once(':') else {
            return false;
        };
        name.eq_ignore_ascii_case("transfer-encoding")
            && value
                .split(',')
                .any(|encoding| encoding.trim().eq_ignore_ascii_case("chunked"))
    })
}

fn decode_chunked_body(bytes: &[u8]) -> Result<String, ManagementApiError> {
    let mut cursor = 0;
    let mut decoded = Vec::new();

    loop {
        let size_line_end = find_crlf(bytes, cursor)
            .ok_or_else(|| ManagementApiError::Http("chunked 响应缺少分块长度".to_string()))?;
        let size_line = std::str::from_utf8(&bytes[cursor..size_line_end]).map_err(|error| {
            ManagementApiError::Http(format!("chunked 长度不是有效 UTF-8：{}", error))
        })?;
        let size_text = size_line.split(';').next().unwrap_or_default().trim();
        let size = usize::from_str_radix(size_text, 16)
            .map_err(|error| ManagementApiError::Http(format!("chunked 长度无效：{}", error)))?;
        cursor = size_line_end + 2;

        if size == 0 {
            return String::from_utf8(decoded).map_err(|error| {
                ManagementApiError::Http(format!("chunked 响应不是有效 UTF-8：{}", error))
            });
        }

        let chunk_end = cursor
            .checked_add(size)
            .ok_or_else(|| ManagementApiError::Http("chunked 响应长度溢出".to_string()))?;
        if chunk_end + 2 > bytes.len() {
            return Err(ManagementApiError::Http(
                "chunked 响应正文不完整".to_string(),
            ));
        }
        decoded.extend_from_slice(&bytes[cursor..chunk_end]);
        if &bytes[chunk_end..chunk_end + 2] != b"\r\n" {
            return Err(ManagementApiError::Http(
                "chunked 响应分块结尾无效".to_string(),
            ));
        }
        cursor = chunk_end + 2;
    }
}

fn find_crlf(bytes: &[u8], start: usize) -> Option<usize> {
    bytes
        .get(start..)?
        .windows(2)
        .position(|pair| pair == b"\r\n")
        .map(|position| start + position)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        net::{TcpListener, TcpStream},
        sync::{Arc, Mutex},
        thread,
        time::Instant,
    };

    #[test]
    fn parse_http_response_decodes_chunked_body() {
        let body = parse_http_response(
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nContent-Type: application/json\r\n\r\ne\r\n{\"debug\":true}\r\n0\r\n\r\n",
        )
        .expect("chunked response should decode");

        assert_eq!(body, r#"{"debug":true}"#);
    }

    #[tokio::test]
    async fn sends_bearer_auth_and_decodes_debug_response() {
        let server = FakeManagementServer::new(vec![FakeResponse::json(200, r#"{"debug":true}"#)]);
        let client = ManagementApiClient::local(server.base_url(), "secret-key");

        let debug = client
            .get_debug()
            .await
            .expect("debug response should decode");

        assert!(debug);
        let requests = server.requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].method, "GET");
        assert_eq!(requests[0].path, "/debug");
        assert!(requests[0]
            .headers
            .iter()
            .any(|header| header.eq_ignore_ascii_case("Authorization: Bearer secret-key")));
    }

    #[tokio::test]
    async fn falls_back_to_legacy_routing_endpoint_on_404() {
        let server = FakeManagementServer::new(vec![
            FakeResponse::json(404, r#"{"error":"missing"}"#),
            FakeResponse::json(200, r#"{"strategy":"round-robin"}"#),
        ]);
        let client = ManagementApiClient::local(server.base_url(), "secret-key");

        let strategy = client
            .get_routing_strategy()
            .await
            .expect("legacy routing response should decode");

        assert_eq!(strategy, "round-robin");
        let requests = server.requests();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0].path, "/routing/strategy");
        assert_eq!(requests[1].path, "/routing");
    }

    #[tokio::test]
    async fn refresh_snapshot_fetches_core_management_payloads() {
        let server = FakeManagementServer::new(vec![
            FakeResponse::json(
                200,
                r#"{"files":[{"id":"claude-1","name":"claude-user.json","provider":"claude","status":"ready","disabled":false,"unavailable":false}]}"#,
            ),
            FakeResponse::json(
                200,
                r#"{"usage":{"total_requests":5,"success_count":4},"failed_requests":1}"#,
            ),
            FakeResponse::json(200, r#"{"api-keys":["sk-local"]}"#),
            FakeResponse::json(200, r#"{"debug":false,"routing-strategy":"round-robin"}"#),
            FakeResponse::json(
                200,
                r#"{"lines":["started"],"line-count":1,"latest-timestamp":10}"#,
            ),
        ]);
        let client = ManagementApiClient::local(server.base_url(), "secret-key");

        let snapshot = client
            .refresh_snapshot()
            .await
            .expect("snapshot should decode");

        assert_eq!(snapshot.auth_files.len(), 1);
        assert_eq!(snapshot.api_keys, vec!["sk-local"]);
        assert_eq!(
            snapshot
                .usage
                .as_ref()
                .and_then(|stats| stats.usage.as_ref())
                .and_then(|usage| usage.total_requests),
            Some(5)
        );
        assert_eq!(
            snapshot
                .config
                .as_ref()
                .and_then(|config| config.routing_strategy.as_deref()),
            Some("round-robin")
        );
        assert_eq!(
            snapshot
                .logs
                .as_ref()
                .and_then(|logs| logs.lines.as_ref())
                .and_then(|lines| lines.first())
                .map(String::as_str),
            Some("started")
        );

        let paths = server
            .requests()
            .into_iter()
            .map(|request| request.path)
            .collect::<Vec<_>>();
        assert_eq!(
            paths,
            vec!["/auth-files", "/usage", "/api-keys", "/config", "/logs"]
        );
    }

    #[tokio::test]
    async fn refresh_snapshot_keeps_core_payloads_when_optional_payloads_are_missing() {
        let server = FakeManagementServer::new(vec![
            FakeResponse::json(
                200,
                r#"{"files":[{"id":"claude-1","name":"claude-user.json","provider":"claude","status":"ready","disabled":false,"unavailable":false}]}"#,
            ),
            FakeResponse::json(404, r#"{"error":"missing"}"#),
            FakeResponse::json(200, r#"{"api-keys":["sk-local"]}"#),
            FakeResponse::json(200, r#"{"debug":false,"routing-strategy":"round-robin"}"#),
            FakeResponse::json(400, r#"{"error":"bad logs query"}"#),
        ]);
        let client = ManagementApiClient::local(server.base_url(), "secret-key");

        let snapshot = client
            .refresh_snapshot()
            .await
            .expect("snapshot should tolerate optional endpoint differences");

        assert_eq!(snapshot.auth_files.len(), 1);
        assert_eq!(snapshot.api_keys, vec!["sk-local"]);
        assert!(snapshot.usage.is_none());
        assert!(snapshot.logs.is_none());
        assert_eq!(
            snapshot
                .config
                .as_ref()
                .and_then(|config| config.routing_strategy.as_deref()),
            Some("round-robin")
        );
    }

    #[tokio::test]
    async fn refresh_snapshot_tolerates_auth_file_count_response() {
        let server = FakeManagementServer::new(vec![
            FakeResponse::json(200, r#"2"#),
            FakeResponse::json(404, r#"{"error":"missing"}"#),
            FakeResponse::json(200, r#"{"api-keys":["sk-local"]}"#),
            FakeResponse::json(200, r#"{"debug":false,"routing-strategy":"round-robin"}"#),
            FakeResponse::json(400, r#"{"error":"bad logs query"}"#),
        ]);
        let client = ManagementApiClient::local(server.base_url(), "secret-key");

        let snapshot = client
            .refresh_snapshot()
            .await
            .expect("snapshot should tolerate auth file count responses");

        assert!(snapshot.auth_files.is_empty());
        assert_eq!(snapshot.api_keys, vec!["sk-local"]);
        assert_eq!(
            snapshot
                .config
                .as_ref()
                .and_then(|config| config.routing_strategy.as_deref()),
            Some("round-robin")
        );
    }

    #[tokio::test]
    async fn write_requests_send_expected_methods_paths_and_bodies() {
        let server = FakeManagementServer::new(vec![
            FakeResponse::json(200, r#"{}"#),
            FakeResponse::json(200, r#"{}"#),
        ]);
        let client = ManagementApiClient::local(server.base_url(), "secret-key");

        client
            .set_proxy_url("http://127.0.0.1:7890")
            .await
            .expect("proxy url should update");
        client
            .replace_api_keys(vec!["sk-a".to_string(), "sk-b".to_string()])
            .await
            .expect("api keys should update");

        let requests = server.requests();
        assert_eq!(requests[0].method, "PUT");
        assert_eq!(requests[0].path, "/proxy-url");
        assert_eq!(requests[0].body, r#"{"value":"http://127.0.0.1:7890"}"#);
        assert_eq!(requests[1].method, "PUT");
        assert_eq!(requests[1].path, "/api-keys");
        assert_eq!(requests[1].body, r#"["sk-a","sk-b"]"#);
    }

    #[derive(Clone, Debug)]
    struct RecordedRequest {
        method: String,
        path: String,
        headers: Vec<String>,
        body: String,
    }

    #[derive(Clone, Debug)]
    struct FakeResponse {
        status: u16,
        body: String,
    }

    impl FakeResponse {
        fn json(status: u16, body: &str) -> Self {
            Self {
                status,
                body: body.to_string(),
            }
        }
    }

    struct FakeManagementServer {
        address: String,
        requests: Arc<Mutex<Vec<RecordedRequest>>>,
        handle: Option<thread::JoinHandle<()>>,
    }

    impl FakeManagementServer {
        fn new(responses: Vec<FakeResponse>) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").expect("fake server should bind");
            let address = listener.local_addr().unwrap().to_string();
            listener.set_nonblocking(true).unwrap();

            let requests = Arc::new(Mutex::new(Vec::new()));
            let captured_requests = Arc::clone(&requests);
            let expected_count = responses.len();

            let handle = thread::spawn(move || {
                let deadline = Instant::now() + Duration::from_secs(5);
                let mut responses = responses.into_iter();

                while captured_requests.lock().unwrap().len() < expected_count
                    && Instant::now() < deadline
                {
                    match listener.accept() {
                        Ok((mut stream, _)) => {
                            stream
                                .set_nonblocking(false)
                                .expect("accepted stream should use blocking reads");
                            let Some(response) = responses.next() else {
                                break;
                            };
                            let request = read_request(&mut stream);
                            captured_requests.lock().unwrap().push(request);
                            write_response(&mut stream, response);
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(10));
                        }
                        Err(_) => break,
                    }
                }
            });

            Self {
                address,
                requests,
                handle: Some(handle),
            }
        }

        fn base_url(&self) -> String {
            format!("http://{}", self.address)
        }

        fn requests(&self) -> Vec<RecordedRequest> {
            self.requests.lock().unwrap().clone()
        }
    }

    impl Drop for FakeManagementServer {
        fn drop(&mut self) {
            if let Some(handle) = self.handle.take() {
                let _ = handle.join();
            }
        }
    }

    fn read_request(stream: &mut TcpStream) -> RecordedRequest {
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .expect("read timeout should be set");
        let mut buffer = [0_u8; 4096];
        let size = stream
            .read(&mut buffer)
            .expect("request should be readable");
        let raw = String::from_utf8_lossy(&buffer[..size]);
        let (head, body) = raw.split_once("\r\n\r\n").unwrap_or((&raw, ""));
        let mut lines = head.lines();
        let request_line = lines.next().unwrap_or_default();
        let mut request_parts = request_line.split_whitespace();
        let method = request_parts.next().unwrap_or_default().to_string();
        let path = request_parts.next().unwrap_or_default().to_string();
        let headers = lines.map(ToOwned::to_owned).collect();

        RecordedRequest {
            method,
            path,
            headers,
            body: body.to_string(),
        }
    }

    fn write_response(stream: &mut TcpStream, response: FakeResponse) {
        let status_text = match response.status {
            200 => "OK",
            404 => "Not Found",
            _ => "Error",
        };
        let payload = format!(
            "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            response.status,
            status_text,
            response.body.len(),
            response.body
        );
        stream
            .write_all(payload.as_bytes())
            .expect("response should be writable");
    }
}
