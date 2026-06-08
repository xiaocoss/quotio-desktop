export type AppSection = "dashboard" | "providers" | "quota" | "agents" | "fallback" | "api_keys" | "logs" | "settings" | "about";
export type OperatingMode = "full" | "quota_only" | "remote";
export type ConnectionMode = "local" | "remote";
export type ThemeMode = "system" | "light" | "dark";
export type ProxyStatusKind = "stopped" | "starting" | "running" | "stopping" | "missing_binary" | "crashed" | "error";
export type RoutingStrategy = "round-robin" | "fill-first";
export type ProviderRole = "provider" | "monitor";
export type AuthMethod = "o_auth" | "api_key" | "service_account" | "local_scan" | "local_token" | "cli";

export type PlatformInfo = {
  os: string;
  family: string;
  arch: string;
};

export type AppSettings = {
  operating_mode: OperatingMode;
  connection_mode: ConnectionMode;
  proxy_host: string;
  proxy_port: number;
  allow_remote: boolean;
  launch_at_login: boolean;
  notifications_enabled: boolean;
  theme: ThemeMode;
  language: string;
  routing_strategy: RoutingStrategy;
  debug: boolean;
  proxy_url: string;
  logging_to_file: boolean;
  logs_max_total_size_mb: number;
  session_affinity: boolean;
  session_affinity_ttl: string;
  max_retry_credentials: number;
  disable_cooling: boolean;
  disable_image_generation: boolean;
  force_model_prefix: boolean;
  passthrough_headers: boolean;
  reasoning_effort: string;
  force_model: string;
  request_retry: number;
  max_retry_interval_seconds: number;
  remote_endpoint_url: string | null;
  remote_management_key: string | null;
};

export type ProxyHealthState = {
  ok: boolean | null;
  checked_at_unix_seconds: number | null;
  message: string;
};

export type ProxyState = {
  status: ProxyStatusKind;
  endpoint: string;
  management_endpoint: string;
  pid: number | null;
  binary_path: string | null;
  config_path: string | null;
  auth_dir: string | null;
  resource_dir: string | null;
  exit_code: number | null;
  crash_count: number;
  health: ProxyHealthState;
  message: string;
};

export type ProxyPlatformResourceStatus = {
  platform: string;
  directory: string;
  files: string[];
  has_binary: boolean;
  detected_binary_path: string | null;
};

export type ProxyResourceStatus = {
  current_platform: string;
  resource_root: string;
  current_resource_dir: string;
  expected_binary_names: string[];
  detected_binary_path: string | null;
  has_current_platform_binary: boolean;
  platforms: ProxyPlatformResourceStatus[];
  message: string;
};

export type ProviderSummary = {
  id: string;
  display_name: string;
  auth_method: AuthMethod;
  role: ProviderRole;
  logo_asset_name: string;
  color_hex: string;
  oauth_endpoint: string | null;
  supports_quota_only_mode: boolean;
  supports_manual_auth: boolean;
  uses_browser_auth: boolean;
  uses_cli_quota: boolean;
  uses_api_key_auth: boolean;
  enabled: boolean;
};

export type AuthFile = {
  id: string;
  name: string;
  provider: string;
  label?: string | null;
  status: string;
  status_message?: string | null;
  disabled: boolean;
  unavailable: boolean;
  runtime_only?: boolean | null;
  source?: string | null;
  path?: string | null;
  email?: string | null;
  account_type?: string | null;
  account?: string | null;
  auth_index?: string | null;
  created_at?: string | null;
  updated_at?: string | null;
  last_refresh?: string | null;
  active_in_ide?: boolean | null;
};

export type ManagementUsageData = {
  total_requests: number | null;
  success_count: number | null;
  failure_count: number | null;
  total_tokens: number | null;
  input_tokens: number | null;
  output_tokens: number | null;
};

export type ManagementUsageStats = {
  usage: ManagementUsageData | null;
  failed_requests: number | null;
};

export type RemoteProxyQuotaExceededConfig = {
  switch_project: boolean | null;
  switch_preview_model: boolean | null;
};

export type RemoteProxyConfig = {
  debug: boolean | null;
  proxy_url: string | null;
  routing_strategy: string | null;
  request_retry: number | null;
  max_retry_interval: number | null;
  logging_to_file: boolean | null;
  request_log: boolean | null;
  quota_exceeded: RemoteProxyQuotaExceededConfig | null;
};

export type ManagementLogs = {
  lines: string[] | null;
  line_count: number | null;
  latest_timestamp: number | null;
};

export type ManagementSnapshot = {
  auth_files: AuthFile[];
  usage: ManagementUsageStats | null;
  api_keys: string[];
  config: RemoteProxyConfig | null;
  logs: ManagementLogs | null;
  latest_version: { latest_version: string } | null;
};

export type OAuthUrlResponse = {
  status: string;
  url: string | null;
  state: string | null;
  error: string | null;
};

export type OAuthStatusResponse = {
  status: string;
  error: string | null;
};

export type AgentConfigType = "environment" | "file" | "both";
export type AgentConfigMode = "automatic" | "manual";
export type AgentSetupMode = "proxy" | "default";
export type AgentConfigStorageOption = "json" | "shell" | "both";
export type ModelSlot = "opus" | "sonnet" | "haiku";
export type RawConfigFormat = "shell" | "toml" | "json" | "yaml" | "text";

export type AvailableModel = {
  id: string;
  name: string;
  provider: string;
  is_default: boolean;
};

export type RawAgentConfigOutput = {
  format: RawConfigFormat;
  content: string;
  filename: string | null;
  target_path: string | null;
  instructions: string;
};

export type AgentBackupFile = {
  path: string;
  timestamp_unix_seconds: number;
  agent_id: string;
  display_name: string;
};

export type SavedAgentConfiguration = {
  agent_id: string;
  base_url: string | null;
  api_key_masked: string | null;
  model_slots: Partial<Record<ModelSlot, string>>;
  is_proxy_configured: boolean;
  backups: AgentBackupFile[];
};

export type AgentConfigurationRequest = {
  agent_id: string;
  mode: AgentConfigMode;
  setup_mode: AgentSetupMode;
  storage_option: AgentConfigStorageOption;
  proxy_url: string;
  api_key: string;
  model_slots: Partial<Record<ModelSlot, string>>;
  use_oauth: boolean;
  available_models: AvailableModel[];
};

export type AgentConfigurationResult = {
  success: boolean;
  config_type: AgentConfigType;
  mode: AgentConfigMode;
  config_path: string | null;
  auth_path: string | null;
  shell_config: string | null;
  raw_configs: RawAgentConfigOutput[];
  instructions: string;
  models_configured: number;
  error: string | null;
  backup_path: string | null;
  backups: AgentBackupFile[];
};

export type CliAgentSummary = {
  id: string;
  display_name: string;
  description: string;
  config_type: AgentConfigType;
  binary_names: string[];
  config_paths: string[];
  docs_url: string | null;
};

export type AgentStatus = {
  agent: CliAgentSummary;
  installed: boolean;
  configured: boolean;
  binary_path: string | null;
  version: string | null;
  last_configured: string | null;
};

export type ApiKeyEntry = {
  value: string;
  masked_value: string;
  source: string;
};

export type FallbackEntry = {
  id: string;
  provider_id: string;
  model_id: string;
  priority: number;
};

export type VirtualModel = {
  id: string;
  name: string;
  fallback_entries: FallbackEntry[];
  is_enabled: boolean;
};

export type FallbackConfiguration = {
  is_enabled: boolean;
  is_route_caching_enabled: boolean;
  virtual_models: VirtualModel[];
};

export type FallbackRouteState = {
  virtual_model_name: string;
  current_entry_index: number;
  current_entry: FallbackEntry;
  last_updated_unix_seconds: number;
  total_entries: number;
};

export type FallbackRuntimeState = {
  route_states: FallbackRouteState[];
  available_models: AvailableModel[];
  model_discovery_status: string;
};

export type FallbackConfigAction =
  | { set_enabled: { enabled: boolean } }
  | { set_route_caching: { enabled: boolean } }
  | { add_virtual_model: { name: string } }
  | { rename_virtual_model: { id: string; name: string } }
  | { remove_virtual_model: { id: string } }
  | { toggle_virtual_model: { id: string; enabled: boolean } }
  | { add_entry: { virtual_model_id: string; provider_id: string; model_id: string } }
  | { remove_entry: { virtual_model_id: string; entry_id: string } }
  | { move_entry: { virtual_model_id: string; entry_id: string; direction: "up" | "down" } }
  | "reset";

export type RequestStats = {
  total_requests: number;
  successful_requests: number;
  failed_requests: number;
  total_input_tokens: number;
  total_output_tokens: number;
  average_duration_ms: number;
};

export type CredentialAvailability = "available" | "unavailable" | "unknown";

export type CredentialStatus = {
  availability: CredentialAvailability;
  local_management_key_configured: boolean;
  remote_management_key_configured: boolean;
  remote_management_key_masked: string | null;
  message: string;
};

export type PlatformFeatureState = {
  launch_at_login_available: boolean;
  launch_at_login_enabled: boolean;
  notifications_available: boolean;
  notifications_enabled: boolean;
  file_manager_available: boolean;
  message: string;
};

export type QuotaTone = "good" | "warn" | "bad" | "neutral";

// Mirrors crates/quotio-types `QuotaModelUsage`. `count` is a UI-only hint the
// backend does not currently populate.
export type QuotaModelUsage = {
  model: string;
  used_percent: number;
  remaining_percent: number;
  reset_at: string | null;
  count?: number | null;
};

// Mirrors crates/quotio-types `AccountQuota`. `account_type` / `warming_up` /
// `in_use` are optional presentation hints not yet sent by the backend.
export type AccountQuota = {
  provider_id: string;
  account_label: string;
  account_key: string;
  is_forbidden: boolean;
  status_message?: string | null;
  models: QuotaModelUsage[];
  account_type?: string | null;
  warming_up?: boolean | null;
  in_use?: boolean | null;
};

// Mirrors crates/quotio-types `RequestLogEntry`.
export type RequestLogEntry = {
  id: string;
  timestamp: string;
  method: string;
  endpoint: string;
  provider?: string | null;
  model?: string | null;
  resolved_model?: string | null;
  resolved_provider?: string | null;
  input_tokens?: number | null;
  output_tokens?: number | null;
  duration_ms: number;
  status_code?: number | null;
  request_size: number;
  response_size: number;
  error_message?: string | null;
  fallback_attempts?: unknown[] | null;
  fallback_started_from_cache: boolean;
  reasoning_effort?: string | null;
  account?: string | null;
};

export type AppState = {
  migration_phase: string;
  platform: PlatformInfo;
  settings: AppSettings;
  proxy: ProxyState;
  proxy_resources: ProxyResourceStatus;
  providers: ProviderSummary[];
  management: ManagementSnapshot;
  auth_files: AuthFile[];
  quotas: AccountQuota[];
  logs: RequestLogEntry[];
  agents: AgentStatus[];
  api_keys: ApiKeyEntry[];
  request_stats: RequestStats | null;
  fallback: FallbackConfiguration;
  fallback_runtime: FallbackRuntimeState;
  credentials: CredentialStatus;
  platform_features: PlatformFeatureState;
  config_root: string;
};

export type ProxyCommand = "start_proxy" | "stop_proxy" | "restart_proxy" | "check_proxy_health" | "download_proxy_binary";