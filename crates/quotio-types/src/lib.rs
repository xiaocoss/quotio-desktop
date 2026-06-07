use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OperatingMode {
    Full,
    QuotaOnly,
    Remote,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionMode {
    Local,
    Remote,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ThemeMode {
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProxyStatusKind {
    Stopped,
    Starting,
    Running,
    Stopping,
    MissingBinary,
    Crashed,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProxyHealthState {
    pub ok: Option<bool>,
    pub checked_at_unix_seconds: Option<u64>,
    pub message: String,
}

impl ProxyHealthState {
    pub fn unknown(message: impl Into<String>) -> Self {
        Self {
            ok: None,
            checked_at_unix_seconds: None,
            message: message.into(),
        }
    }

    pub fn healthy(checked_at_unix_seconds: u64, message: impl Into<String>) -> Self {
        Self {
            ok: Some(true),
            checked_at_unix_seconds: Some(checked_at_unix_seconds),
            message: message.into(),
        }
    }

    pub fn unhealthy(checked_at_unix_seconds: u64, message: impl Into<String>) -> Self {
        Self {
            ok: Some(false),
            checked_at_unix_seconds: Some(checked_at_unix_seconds),
            message: message.into(),
        }
    }
}

impl Default for ProxyHealthState {
    fn default() -> Self {
        Self::unknown("尚未执行健康检查。")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum RoutingStrategy {
    RoundRobin,
    FillFirst,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MigrationPhase {
    Bootstrap,
    ProxyCore,
    ManagementApi,
    UiMigration,
    PlatformAdapters,
    Packaging,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlatformInfo {
    pub os: String,
    pub family: String,
    pub arch: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct AppSettings {
    pub operating_mode: OperatingMode,
    pub connection_mode: ConnectionMode,
    pub proxy_host: String,
    pub proxy_port: u16,
    pub allow_remote: bool,
    pub launch_at_login: bool,
    pub notifications_enabled: bool,
    pub theme: ThemeMode,
    pub language: String,
    pub routing_strategy: RoutingStrategy,
    pub debug: bool,
    pub proxy_url: String,
    pub logging_to_file: bool,
    pub logs_max_total_size_mb: u32,
    pub session_affinity: bool,
    pub session_affinity_ttl: String,
    pub max_retry_credentials: u32,
    pub disable_cooling: bool,
    pub disable_image_generation: bool,
    pub force_model_prefix: bool,
    pub passthrough_headers: bool,
    pub reasoning_effort: String,
    pub force_model: String,
    pub request_retry: u8,
    pub max_retry_interval_seconds: u16,
    pub remote_endpoint_url: Option<String>,
    pub remote_management_key: Option<String>,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            operating_mode: OperatingMode::Full,
            connection_mode: ConnectionMode::Local,
            proxy_host: "127.0.0.1".to_string(),
            proxy_port: 28317,
            allow_remote: false,
            launch_at_login: false,
            notifications_enabled: true,
            theme: ThemeMode::System,
            language: "system".to_string(),
            routing_strategy: RoutingStrategy::RoundRobin,
            debug: false,
            proxy_url: String::new(),
            logging_to_file: false,
            logs_max_total_size_mb: 0,
            session_affinity: false,
            session_affinity_ttl: "1h".to_string(),
            max_retry_credentials: 0,
            disable_cooling: false,
            disable_image_generation: false,
            force_model_prefix: false,
            passthrough_headers: false,
            reasoning_effort: String::new(),
            force_model: String::new(),
            request_retry: 3,
            max_retry_interval_seconds: 30,
            remote_endpoint_url: None,
            remote_management_key: None,
        }
    }
}

impl AppSettings {
    pub fn endpoint(&self) -> String {
        format!("http://{}:{}", self.proxy_host, self.proxy_port)
    }

    pub fn management_endpoint(&self) -> String {
        match (&self.connection_mode, &self.remote_endpoint_url) {
            (ConnectionMode::Remote, Some(endpoint)) if !endpoint.trim().is_empty() => {
                normalize_management_endpoint(endpoint)
            }
            _ => format!("{}/v0/management", self.endpoint()),
        }
    }
}

pub fn normalize_management_endpoint(value: &str) -> String {
    let mut url = value.trim().trim_end_matches('/').to_string();
    if url.ends_with("/v0/management") {
        return url;
    }
    if url.ends_with("/v0") {
        url.push_str("/management");
        return url;
    }
    url.push_str("/v0/management");
    url
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProxyState {
    pub status: ProxyStatusKind,
    pub endpoint: String,
    pub management_endpoint: String,
    pub pid: Option<u32>,
    pub binary_path: Option<String>,
    pub config_path: Option<String>,
    pub auth_dir: Option<String>,
    pub resource_dir: Option<String>,
    pub exit_code: Option<i32>,
    pub crash_count: u32,
    pub health: ProxyHealthState,
    pub message: String,
}

impl ProxyState {
    pub fn stopped(settings: &AppSettings) -> Self {
        Self {
            status: ProxyStatusKind::Stopped,
            endpoint: settings.endpoint(),
            management_endpoint: settings.management_endpoint(),
            pid: None,
            binary_path: None,
            config_path: None,
            auth_dir: None,
            resource_dir: None,
            exit_code: None,
            crash_count: 0,
            health: ProxyHealthState::default(),
            message: "代理核心尚未启动。".to_string(),
        }
    }

    pub fn missing_binary(settings: &AppSettings, expected_path: String) -> Self {
        Self {
            status: ProxyStatusKind::MissingBinary,
            endpoint: settings.endpoint(),
            management_endpoint: settings.management_endpoint(),
            pid: None,
            binary_path: Some(expected_path),
            config_path: None,
            auth_dir: None,
            resource_dir: None,
            exit_code: None,
            crash_count: 0,
            health: ProxyHealthState::unknown("缺少可执行文件，无法检查健康状态。"),
            message: "未找到当前平台可用的 CLIProxyAPI 二进制。".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProxyPlatformResourceStatus {
    pub platform: String,
    pub directory: String,
    pub files: Vec<String>,
    pub has_binary: bool,
    pub detected_binary_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProxyResourceStatus {
    pub current_platform: String,
    pub resource_root: String,
    pub current_resource_dir: String,
    pub expected_binary_names: Vec<String>,
    pub detected_binary_path: Option<String>,
    pub has_current_platform_binary: bool,
    pub platforms: Vec<ProxyPlatformResourceStatus>,
    pub message: String,
}

impl Default for ProxyResourceStatus {
    fn default() -> Self {
        Self {
            current_platform: String::new(),
            resource_root: String::new(),
            current_resource_dir: String::new(),
            expected_binary_names: Vec::new(),
            detected_binary_path: None,
            has_current_platform_binary: false,
            platforms: Vec::new(),
            message: "尚未检查代理资源。".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProviderRole {
    Provider,
    Monitor,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AuthMethod {
    OAuth,
    ApiKey,
    ServiceAccount,
    LocalScan,
    LocalToken,
    Cli,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProviderSummary {
    pub id: String,
    pub display_name: String,
    pub auth_method: AuthMethod,
    pub role: ProviderRole,
    pub logo_asset_name: String,
    pub color_hex: String,
    pub oauth_endpoint: Option<String>,
    pub supports_quota_only_mode: bool,
    pub supports_manual_auth: bool,
    pub uses_browser_auth: bool,
    pub uses_cli_quota: bool,
    pub uses_api_key_auth: bool,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AuthFile {
    pub id: String,
    pub name: String,
    pub provider: String,
    pub label: Option<String>,
    pub status: String,
    pub status_message: Option<String>,
    pub disabled: bool,
    pub unavailable: bool,
    pub runtime_only: Option<bool>,
    pub source: Option<String>,
    pub path: Option<String>,
    pub email: Option<String>,
    pub account_type: Option<String>,
    pub account: Option<String>,
    pub auth_index: Option<String>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
    pub last_refresh: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AuthFilesResponse {
    pub files: Vec<AuthFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AuthFileModelInfo {
    pub id: String,
    pub owned_by: Option<String>,
    #[serde(rename = "type")]
    pub model_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AuthFileModelsResponse {
    pub models: Vec<AuthFileModelInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct QuotaModelUsage {
    pub model: String,
    pub used_percent: f64,
    pub remaining_percent: f64,
    pub reset_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AccountQuota {
    pub provider_id: String,
    pub account_label: String,
    pub account_key: String,
    pub is_forbidden: bool,
    pub status_message: Option<String>,
    pub models: Vec<QuotaModelUsage>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UsageStats {
    pub usage: Option<UsageData>,
    pub failed_requests: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UsageData {
    pub total_requests: Option<u64>,
    pub success_count: Option<u64>,
    pub failure_count: Option<u64>,
    pub total_tokens: Option<u64>,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ApiKeysResponse {
    #[serde(rename = "api-keys")]
    pub api_keys: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ApiKeyEntry {
    pub value: String,
    pub masked_value: String,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OAuthUrlResponse {
    pub status: String,
    pub url: Option<String>,
    pub state: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OAuthStatusResponse {
    pub status: String,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LogsResponse {
    pub lines: Option<Vec<String>>,
    #[serde(rename = "line-count")]
    pub line_count: Option<u64>,
    #[serde(rename = "latest-timestamp")]
    pub latest_timestamp: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FallbackAttemptOutcome {
    Failed,
    Success,
    Skipped,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum FallbackTriggerReason {
    HttpStatus(u16),
    Pattern(String),
    CachedRoute,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FallbackAttempt {
    pub provider: String,
    pub model_id: String,
    pub outcome: FallbackAttemptOutcome,
    pub reason: Option<FallbackTriggerReason>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RequestLogEntry {
    pub id: String,
    pub timestamp: String,
    pub method: String,
    pub endpoint: String,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub resolved_model: Option<String>,
    pub resolved_provider: Option<String>,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub duration_ms: u64,
    pub status_code: Option<u16>,
    pub request_size: u64,
    pub response_size: u64,
    pub error_message: Option<String>,
    pub fallback_attempts: Option<Vec<FallbackAttempt>>,
    pub fallback_started_from_cache: bool,
    #[serde(default)]
    pub reasoning_effort: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RequestStats {
    pub total_requests: u64,
    pub successful_requests: u64,
    pub failed_requests: u64,
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub average_duration_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentConfigType {
    Environment,
    File,
    Both,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CliAgentSummary {
    pub id: String,
    pub display_name: String,
    pub description: String,
    pub config_type: AgentConfigType,
    pub binary_names: Vec<String>,
    pub config_paths: Vec<String>,
    pub docs_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentStatus {
    pub agent: CliAgentSummary,
    pub installed: bool,
    pub configured: bool,
    pub binary_path: Option<String>,
    pub version: Option<String>,
    pub last_configured: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentConfigMode {
    Automatic,
    Manual,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentSetupMode {
    Proxy,
    Default,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentConfigStorageOption {
    Json,
    Shell,
    Both,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "snake_case")]
pub enum ModelSlot {
    Opus,
    Sonnet,
    Haiku,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AvailableModel {
    pub id: String,
    pub name: String,
    pub provider: String,
    pub is_default: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RawConfigFormat {
    Shell,
    Toml,
    Json,
    Yaml,
    Text,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RawAgentConfigOutput {
    pub format: RawConfigFormat,
    pub content: String,
    pub filename: Option<String>,
    pub target_path: Option<String>,
    pub instructions: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentBackupFile {
    pub path: String,
    pub timestamp_unix_seconds: u64,
    pub agent_id: String,
    pub display_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SavedAgentConfiguration {
    pub agent_id: String,
    pub base_url: Option<String>,
    pub api_key_masked: Option<String>,
    pub model_slots: std::collections::BTreeMap<ModelSlot, String>,
    pub is_proxy_configured: bool,
    pub backups: Vec<AgentBackupFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentConfigurationRequest {
    pub agent_id: String,
    pub mode: AgentConfigMode,
    pub setup_mode: AgentSetupMode,
    pub storage_option: AgentConfigStorageOption,
    pub proxy_url: String,
    pub api_key: String,
    pub model_slots: std::collections::BTreeMap<ModelSlot, String>,
    pub use_oauth: bool,
    pub available_models: Vec<AvailableModel>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentConfigurationResult {
    pub success: bool,
    pub config_type: AgentConfigType,
    pub mode: AgentConfigMode,
    pub config_path: Option<String>,
    pub auth_path: Option<String>,
    pub shell_config: Option<String>,
    pub raw_configs: Vec<RawAgentConfigOutput>,
    pub instructions: String,
    pub models_configured: usize,
    pub error: Option<String>,
    pub backup_path: Option<String>,
    pub backups: Vec<AgentBackupFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ModelType {
    Claude,
    Gpt,
    Gemini,
    Compatible,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FallbackEntry {
    pub id: String,
    pub provider_id: String,
    pub model_id: String,
    pub priority: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VirtualModel {
    pub id: String,
    pub name: String,
    pub fallback_entries: Vec<FallbackEntry>,
    pub is_enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FallbackConfiguration {
    pub is_enabled: bool,
    pub is_route_caching_enabled: bool,
    pub virtual_models: Vec<VirtualModel>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FallbackRouteState {
    pub virtual_model_name: String,
    pub current_entry_index: usize,
    pub current_entry: FallbackEntry,
    pub last_updated_unix_seconds: u64,
    pub total_entries: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FallbackRuntimeState {
    pub route_states: Vec<FallbackRouteState>,
    pub available_models: Vec<AvailableModel>,
    pub model_discovery_status: String,
}

impl Default for FallbackRuntimeState {
    fn default() -> Self {
        Self {
            route_states: Vec::new(),
            available_models: default_available_models(),
            model_discovery_status: "using_builtin_defaults".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FallbackConfigAction {
    SetEnabled {
        enabled: bool,
    },
    SetRouteCaching {
        enabled: bool,
    },
    AddVirtualModel {
        name: String,
    },
    RenameVirtualModel {
        id: String,
        name: String,
    },
    RemoveVirtualModel {
        id: String,
    },
    ToggleVirtualModel {
        id: String,
        enabled: bool,
    },
    AddEntry {
        virtual_model_id: String,
        provider_id: String,
        model_id: String,
    },
    RemoveEntry {
        virtual_model_id: String,
        entry_id: String,
    },
    MoveEntry {
        virtual_model_id: String,
        entry_id: String,
        direction: FallbackEntryMoveDirection,
    },
    Reset,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FallbackEntryMoveDirection {
    Up,
    Down,
}

impl FallbackConfiguration {
    pub fn enabled_model_names(&self) -> Vec<String> {
        self.virtual_models
            .iter()
            .filter(|model| model.is_enabled)
            .map(|model| model.name.clone())
            .collect()
    }
}

impl Default for FallbackConfiguration {
    fn default() -> Self {
        Self {
            is_enabled: false,
            is_route_caching_enabled: true,
            virtual_models: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProxyConfig {
    pub host: String,
    pub port: u16,
    pub auth_dir: String,
    pub proxy_url: Option<String>,
    pub api_keys: Vec<String>,
    pub debug: bool,
    pub logging_to_file: bool,
    pub usage_statistics_enabled: bool,
    pub request_retry: u8,
    pub max_retry_interval: u16,
    pub ws_auth: bool,
    pub routing: RoutingConfig,
    pub quota_exceeded: QuotaExceededConfig,
    pub remote_management: RemoteManagementConfig,
}

impl Default for ProxyConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: 28317,
            auth_dir: "~/.cli-proxy-api".to_string(),
            proxy_url: None,
            api_keys: Vec::new(),
            debug: false,
            logging_to_file: false,
            usage_statistics_enabled: true,
            request_retry: 3,
            max_retry_interval: 30,
            ws_auth: false,
            routing: RoutingConfig::default(),
            quota_exceeded: QuotaExceededConfig::default(),
            remote_management: RemoteManagementConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RoutingConfig {
    pub strategy: RoutingStrategy,
}

impl Default for RoutingConfig {
    fn default() -> Self {
        Self {
            strategy: RoutingStrategy::RoundRobin,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct QuotaExceededConfig {
    pub switch_project: bool,
    pub switch_preview_model: bool,
}

impl Default for QuotaExceededConfig {
    fn default() -> Self {
        Self {
            switch_project: true,
            switch_preview_model: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RemoteManagementConfig {
    pub allow_remote: bool,
    pub management_key: Option<String>,
}

impl Default for RemoteManagementConfig {
    fn default() -> Self {
        Self {
            allow_remote: false,
            management_key: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RemoteProxyConfig {
    pub debug: Option<bool>,
    #[serde(rename = "proxy-url")]
    pub proxy_url: Option<String>,
    #[serde(rename = "routing-strategy")]
    pub routing_strategy: Option<String>,
    #[serde(rename = "request-retry")]
    pub request_retry: Option<u8>,
    #[serde(rename = "max-retry-interval")]
    pub max_retry_interval: Option<u16>,
    #[serde(rename = "logging-to-file")]
    pub logging_to_file: Option<bool>,
    #[serde(rename = "request-log")]
    pub request_log: Option<bool>,
    #[serde(rename = "quota-exceeded")]
    pub quota_exceeded: Option<RemoteProxyQuotaExceededConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RemoteProxyQuotaExceededConfig {
    #[serde(rename = "switch-project")]
    pub switch_project: Option<bool>,
    #[serde(rename = "switch-preview-model")]
    pub switch_preview_model: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BoolValueRequest {
    pub value: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IntegerValueRequest {
    pub value: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StringValueRequest {
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AuthFileStatusRequest {
    pub name: String,
    pub disabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ApiKeyUpdateRequest {
    pub old: String,
    pub new: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct APICallRequest {
    #[serde(rename = "auth_index")]
    pub auth_index: Option<String>,
    pub method: String,
    pub url: String,
    pub header: Option<std::collections::BTreeMap<String, String>>,
    pub data: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct APICallResponse {
    #[serde(rename = "status_code")]
    pub status_code: u16,
    pub header: Option<std::collections::BTreeMap<String, Vec<String>>>,
    pub body: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DebugResponse {
    pub debug: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProxyUrlResponse {
    #[serde(rename = "proxy-url")]
    pub proxy_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LoggingToFileResponse {
    #[serde(rename = "logging-to-file")]
    pub logging_to_file: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RequestLogResponse {
    #[serde(rename = "request-log")]
    pub request_log: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RequestRetryResponse {
    #[serde(rename = "request-retry")]
    pub request_retry: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MaxRetryIntervalResponse {
    #[serde(rename = "max-retry-interval")]
    pub max_retry_interval: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SwitchProjectResponse {
    #[serde(rename = "switch-project")]
    pub switch_project: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SwitchPreviewModelResponse {
    #[serde(rename = "switch-preview-model")]
    pub switch_preview_model: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct RoutingStrategyResponse {
    pub strategy: String,
}

impl<'de> Deserialize<'de> for RoutingStrategyResponse {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct WireResponse {
            strategy: Option<String>,
            value: Option<String>,
        }

        let response = WireResponse::deserialize(deserializer)?;
        let strategy = response
            .strategy
            .or(response.value)
            .ok_or_else(|| serde::de::Error::missing_field("strategy"))?;
        Ok(Self { strategy })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LatestVersionResponse {
    #[serde(rename = "latest-version")]
    pub latest_version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CredentialAvailability {
    Available,
    Unavailable,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CredentialStatus {
    pub availability: CredentialAvailability,
    pub local_management_key_configured: bool,
    pub remote_management_key_configured: bool,
    pub remote_management_key_masked: Option<String>,
    pub message: String,
}

impl Default for CredentialStatus {
    fn default() -> Self {
        Self {
            availability: CredentialAvailability::Unknown,
            local_management_key_configured: false,
            remote_management_key_configured: false,
            remote_management_key_masked: None,
            message: "尚未检查凭据存储。".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlatformFeatureState {
    pub launch_at_login_available: bool,
    pub launch_at_login_enabled: bool,
    pub notifications_available: bool,
    pub notifications_enabled: bool,
    pub file_manager_available: bool,
    pub message: String,
}

impl Default for PlatformFeatureState {
    fn default() -> Self {
        Self {
            launch_at_login_available: false,
            launch_at_login_enabled: false,
            notifications_available: false,
            notifications_enabled: false,
            file_manager_available: true,
            message: "平台适配状态尚未刷新。".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ManagementSnapshot {
    pub auth_files: Vec<AuthFile>,
    pub usage: Option<UsageStats>,
    pub api_keys: Vec<String>,
    pub config: Option<RemoteProxyConfig>,
    pub logs: Option<LogsResponse>,
    pub latest_version: Option<LatestVersionResponse>,
}

impl Default for ManagementSnapshot {
    fn default() -> Self {
        Self {
            auth_files: Vec::new(),
            usage: None,
            api_keys: Vec::new(),
            config: None,
            logs: None,
            latest_version: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AppState {
    pub migration_phase: MigrationPhase,
    pub platform: PlatformInfo,
    pub settings: AppSettings,
    pub proxy: ProxyState,
    pub proxy_resources: ProxyResourceStatus,
    pub providers: Vec<ProviderSummary>,
    pub management: ManagementSnapshot,
    pub auth_files: Vec<AuthFile>,
    pub quotas: Vec<AccountQuota>,
    pub logs: Vec<RequestLogEntry>,
    pub agents: Vec<AgentStatus>,
    pub api_keys: Vec<ApiKeyEntry>,
    pub request_stats: Option<RequestStats>,
    pub fallback: FallbackConfiguration,
    pub fallback_runtime: FallbackRuntimeState,
    pub credentials: CredentialStatus,
    pub platform_features: PlatformFeatureState,
    pub config_root: String,
}

pub fn default_providers() -> Vec<ProviderSummary> {
    vec![
        provider(
            "gemini-cli",
            "Gemini CLI",
            AuthMethod::OAuth,
            ProviderRole::Provider,
            "gemini",
            "4285F4",
            Some("/gemini-cli-auth-url"),
            true,
            true,
            false,
            true,
            false,
        ),
        provider(
            "claude",
            "Claude Code",
            AuthMethod::OAuth,
            ProviderRole::Provider,
            "claude",
            "D97706",
            Some("/anthropic-auth-url"),
            true,
            true,
            false,
            true,
            false,
        ),
        provider(
            "codex",
            "Codex (OpenAI)",
            AuthMethod::OAuth,
            ProviderRole::Provider,
            "openai",
            "10A37F",
            Some("/codex-auth-url"),
            true,
            true,
            false,
            true,
            false,
        ),
        provider(
            "qwen",
            "Qwen Code",
            AuthMethod::OAuth,
            ProviderRole::Provider,
            "qwen",
            "7C3AED",
            Some("/qwen-auth-url"),
            false,
            true,
            false,
            false,
            false,
        ),
        provider(
            "iflow",
            "iFlow",
            AuthMethod::OAuth,
            ProviderRole::Provider,
            "iflow",
            "06B6D4",
            Some("/iflow-auth-url"),
            false,
            true,
            false,
            false,
            false,
        ),
        provider(
            "antigravity",
            "Antigravity",
            AuthMethod::OAuth,
            ProviderRole::Provider,
            "antigravity",
            "EC4899",
            Some("/antigravity-auth-url"),
            true,
            true,
            false,
            false,
            false,
        ),
        provider(
            "vertex",
            "Vertex AI",
            AuthMethod::ServiceAccount,
            ProviderRole::Provider,
            "vertex",
            "EA4335",
            None,
            false,
            true,
            false,
            false,
            false,
        ),
        provider(
            "kiro",
            "Kiro",
            AuthMethod::Cli,
            ProviderRole::Provider,
            "kiro",
            "9046FF",
            None,
            true,
            true,
            false,
            false,
            false,
        ),
        provider(
            "github-copilot",
            "GitHub Copilot",
            AuthMethod::Cli,
            ProviderRole::Provider,
            "copilot",
            "238636",
            None,
            true,
            true,
            false,
            false,
            false,
        ),
        provider(
            "cursor",
            "Cursor",
            AuthMethod::LocalScan,
            ProviderRole::Monitor,
            "cursor",
            "00D4AA",
            None,
            true,
            false,
            true,
            false,
            false,
        ),
        provider(
            "trae",
            "Trae",
            AuthMethod::LocalScan,
            ProviderRole::Monitor,
            "trae",
            "00B4D8",
            None,
            true,
            false,
            true,
            false,
            false,
        ),
        provider(
            "glm",
            "GLM",
            AuthMethod::ApiKey,
            ProviderRole::Provider,
            "glm",
            "3B82F6",
            None,
            true,
            false,
            false,
            false,
            true,
        ),
        provider(
            "warp",
            "Warp",
            AuthMethod::LocalToken,
            ProviderRole::Monitor,
            "warp",
            "01E5FF",
            None,
            true,
            true,
            false,
            false,
            true,
        ),
    ]
}

fn provider(
    id: &str,
    display_name: &str,
    auth_method: AuthMethod,
    role: ProviderRole,
    logo_asset_name: &str,
    color_hex: &str,
    oauth_endpoint: Option<&str>,
    supports_quota_only_mode: bool,
    supports_manual_auth: bool,
    uses_browser_auth: bool,
    uses_cli_quota: bool,
    uses_api_key_auth: bool,
) -> ProviderSummary {
    ProviderSummary {
        id: id.to_string(),
        display_name: display_name.to_string(),
        auth_method,
        role,
        logo_asset_name: logo_asset_name.to_string(),
        color_hex: color_hex.to_string(),
        oauth_endpoint: oauth_endpoint.map(ToOwned::to_owned),
        supports_quota_only_mode,
        supports_manual_auth,
        uses_browser_auth,
        uses_cli_quota,
        uses_api_key_auth,
        enabled: true,
    }
}

pub fn default_cli_agents() -> Vec<CliAgentSummary> {
    vec![
        cli_agent(
            "claude-code",
            "Claude Code",
            "Anthropic's official CLI for Claude models",
            AgentConfigType::Both,
            &["claude"],
            &["~/.claude/settings.json"],
            Some("https://docs.anthropic.com/en/docs/claude-code"),
        ),
        cli_agent(
            "codex",
            "Codex CLI",
            "OpenAI's Codex CLI for GPT-5 models",
            AgentConfigType::File,
            &["codex"],
            &["~/.codex/config.toml", "~/.codex/auth.json"],
            Some("https://github.com/openai/codex"),
        ),
        cli_agent(
            "gemini-cli",
            "Gemini CLI",
            "Google's Gemini CLI for Gemini models",
            AgentConfigType::Environment,
            &["gemini"],
            &[],
            Some("https://github.com/google-gemini/gemini-cli"),
        ),
        cli_agent(
            "amp",
            "Amp CLI",
            "Sourcegraph's Amp coding assistant",
            AgentConfigType::Both,
            &["amp"],
            &[
                "~/.config/amp/settings.json",
                "~/.local/share/amp/secrets.json",
            ],
            Some("https://ampcode.com/manual"),
        ),
        cli_agent(
            "opencode",
            "OpenCode",
            "The open source AI coding agent",
            AgentConfigType::File,
            &["opencode", "oc"],
            &["~/.config/opencode/opencode.json"],
            Some("https://github.com/sst/opencode"),
        ),
        cli_agent(
            "factory-droid",
            "Factory Droid",
            "Factory's AI coding agent",
            AgentConfigType::File,
            &["droid", "factory-droid"],
            &["~/.factory/config.json"],
            Some("https://docs.factory.ai/welcome"),
        ),
    ]
}

fn cli_agent(
    id: &str,
    display_name: &str,
    description: &str,
    config_type: AgentConfigType,
    binary_names: &[&str],
    config_paths: &[&str],
    docs_url: Option<&str>,
) -> CliAgentSummary {
    CliAgentSummary {
        id: id.to_string(),
        display_name: display_name.to_string(),
        description: description.to_string(),
        config_type,
        binary_names: binary_names.iter().map(|value| value.to_string()).collect(),
        config_paths: config_paths.iter().map(|value| value.to_string()).collect(),
        docs_url: docs_url.map(ToOwned::to_owned),
    }
}

pub fn default_model_slots() -> std::collections::BTreeMap<ModelSlot, String> {
    [
        (ModelSlot::Opus, "gemini-claude-opus-4-6-thinking"),
        (ModelSlot::Sonnet, "gemini-claude-sonnet-4-5"),
        (ModelSlot::Haiku, "gemini-3-flash-preview"),
    ]
    .into_iter()
    .map(|(slot, model)| (slot, model.to_string()))
    .collect()
}

pub fn default_available_models() -> Vec<AvailableModel> {
    [
        ("gemini-claude-opus-4-6-thinking", "anthropic", true),
        ("gemini-claude-opus-4-5-thinking", "anthropic", false),
        ("gemini-claude-sonnet-4-5", "anthropic", true),
        ("gemini-claude-sonnet-4-5-thinking", "anthropic", false),
        ("gemini-3-pro-preview", "google", false),
        ("gemini-3-pro-image-preview", "google", false),
        ("gemini-3-flash-preview", "google", true),
        ("gemini-2.5-flash", "google", false),
        ("gemini-2.5-flash-lite", "google", false),
        ("gpt-5.3-codex", "openai", false),
        ("gpt-5.2", "openai", false),
        ("gpt-5.2-codex", "openai", false),
        ("gpt-5.1", "openai", false),
        ("gpt-5.1-codex", "openai", false),
        ("gpt-5.1-codex-max", "openai", false),
        ("gpt-5.1-codex-mini", "openai", false),
        ("gpt-5", "openai", false),
        ("gpt-5-codex", "openai", false),
        ("gpt-5-codex-mini", "openai", false),
        ("gpt-oss-120b-medium", "openai", false),
    ]
    .into_iter()
    .map(|(name, provider, is_default)| AvailableModel {
        id: name.to_string(),
        name: name.to_string(),
        provider: provider.to_string(),
        is_default,
    })
    .collect()
}

pub fn empty_agent_statuses() -> Vec<AgentStatus> {
    default_cli_agents()
        .into_iter()
        .map(|agent| AgentStatus {
            agent,
            installed: false,
            configured: false,
            binary_path: None,
            version: None,
            last_configured: None,
        })
        .collect()
}

pub fn mask_secret(value: &str) -> String {
    let chars: Vec<char> = value.chars().collect();
    if chars.len() <= 8 {
        return "••••".to_string();
    }
    let prefix: String = chars.iter().take(4).collect();
    let suffix: String = chars
        .iter()
        .rev()
        .take(4)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("{}••••{}", prefix, suffix)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn default_local_settings_use_unoccupied_management_port() {
        let settings = AppSettings::default();
        let proxy_config = ProxyConfig::default();

        assert_eq!(settings.proxy_port, 28317);
        assert_eq!(settings.endpoint(), "http://127.0.0.1:28317");
        assert_eq!(settings.management_endpoint(), "http://127.0.0.1:28317/v0/management");
        assert_eq!(proxy_config.port, 28317);
    }

    #[test]
    fn routing_strategy_response_accepts_current_and_legacy_shapes() {
        let current: RoutingStrategyResponse = serde_json::from_value(json!({
            "strategy": "fill-first"
        }))
        .expect("current routing response should decode");

        let legacy: RoutingStrategyResponse = serde_json::from_value(json!({
            "value": "round-robin"
        }))
        .expect("legacy routing response should decode");

        assert_eq!(current.strategy, "fill-first");
        assert_eq!(legacy.strategy, "round-robin");
    }

    #[test]
    fn management_write_requests_keep_proxy_wire_keys() {
        let bool_body = serde_json::to_value(BoolValueRequest { value: true }).unwrap();
        let int_body = serde_json::to_value(IntegerValueRequest { value: 4 }).unwrap();
        let status_body = serde_json::to_value(AuthFileStatusRequest {
            name: "claude-user.json".to_string(),
            disabled: true,
        })
        .unwrap();
        let api_key_body = serde_json::to_value(ApiKeyUpdateRequest {
            old: "sk-old".to_string(),
            new: "sk-new".to_string(),
        })
        .unwrap();

        assert_eq!(bool_body, json!({ "value": true }));
        assert_eq!(int_body, json!({ "value": 4 }));
        assert_eq!(
            status_body,
            json!({ "name": "claude-user.json", "disabled": true })
        );
        assert_eq!(api_key_body, json!({ "old": "sk-old", "new": "sk-new" }));
    }

    #[test]
    fn api_call_contract_uses_management_api_wire_keys() {
        let request = APICallRequest {
            auth_index: Some("gemini-0".to_string()),
            method: "POST".to_string(),
            url: "https://example.test/quota".to_string(),
            header: Some([("content-type".to_string(), "application/json".to_string())].into()),
            data: Some("{}".to_string()),
        };

        let value = serde_json::to_value(request).unwrap();
        assert_eq!(
            value,
            json!({
                "auth_index": "gemini-0",
                "method": "POST",
                "url": "https://example.test/quota",
                "header": { "content-type": "application/json" },
                "data": "{}"
            })
        );

        let response: APICallResponse = serde_json::from_value(json!({
            "status_code": 200,
            "header": { "x-test": ["ok"] },
            "body": "done"
        }))
        .unwrap();

        assert_eq!(response.status_code, 200);
        assert_eq!(response.body.as_deref(), Some("done"));
    }

    #[test]
    fn management_snapshot_collects_first_refresh_payload() {
        let config: RemoteProxyConfig = serde_json::from_value(json!({
            "debug": true,
            "proxy-url": "http://127.0.0.1:7890",
            "routing-strategy": "fill-first",
            "request-retry": 4,
            "max-retry-interval": 45,
            "logging-to-file": true,
            "request-log": false,
            "quota-exceeded": {
                "switch-project": true,
                "switch-preview-model": false
            }
        }))
        .unwrap();

        let snapshot = ManagementSnapshot {
            auth_files: vec![AuthFile {
                id: "claude-1".to_string(),
                name: "claude-user.json".to_string(),
                provider: "claude".to_string(),
                label: None,
                status: "ready".to_string(),
                status_message: None,
                disabled: false,
                unavailable: false,
                runtime_only: None,
                source: None,
                path: None,
                email: Some("user@example.com".to_string()),
                account_type: None,
                account: None,
                auth_index: Some("0".to_string()),
                created_at: None,
                updated_at: None,
                last_refresh: None,
            }],
            usage: Some(UsageStats {
                usage: Some(UsageData {
                    total_requests: Some(10),
                    success_count: Some(9),
                    failure_count: Some(1),
                    total_tokens: Some(1000),
                    input_tokens: Some(600),
                    output_tokens: Some(400),
                }),
                failed_requests: Some(1),
            }),
            api_keys: vec!["sk-test".to_string()],
            config: Some(config),
            logs: Some(LogsResponse {
                lines: Some(vec!["proxy started".to_string()]),
                line_count: Some(1),
                latest_timestamp: Some(123),
            }),
            latest_version: Some(LatestVersionResponse {
                latest_version: "1.2.3".to_string(),
            }),
        };

        assert_eq!(snapshot.auth_files.len(), 1);
        assert_eq!(snapshot.api_keys, vec!["sk-test"]);
        assert_eq!(
            snapshot
                .config
                .as_ref()
                .and_then(|config| config.routing_strategy.as_deref()),
            Some("fill-first")
        );
        assert_eq!(
            snapshot
                .logs
                .as_ref()
                .and_then(|logs| logs.lines.as_ref())
                .and_then(|lines| lines.first())
                .map(String::as_str),
            Some("proxy started")
        );
    }
}
