pub mod agent_config;
pub mod agents;
pub mod bridge;
pub mod management;
pub mod proxy_download;
pub mod quota;
pub mod tunnel;

use std::{
    fs,
    io::{Read as _, Write as _},
    net::{TcpStream, ToSocketAddrs},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use management::{ManagementApiClient, ManagementApiError};
use quotio_types::{
    default_available_models, default_providers, mask_secret, AccountQuota, AuthFile, AgentBackupFile,
    AgentConfigurationRequest, AgentConfigurationResult, ApiKeyEntry, AppSettings, AppState,
    AvailableModel, ConnectionMode, CredentialStatus, FallbackConfigAction, FallbackConfiguration,
    FallbackEntry, FallbackEntryMoveDirection, FallbackRouteState, FallbackRuntimeState,
    ManagementSnapshot, MigrationPhase, PlatformInfo, ProxyHealthState,
    ProxyPlatformResourceStatus, ProxyResourceStatus, ProxyState, ProxyStatusKind, RequestLogEntry,
    RequestStats,
    RoutingStrategy, SavedAgentConfiguration, VirtualModel,
};
use uuid::Uuid;

#[derive(Debug)]
pub enum ProxyCoreError {
    Io {
        context: &'static str,
        source: std::io::Error,
    },
    StartupFailed(String),
}

impl std::fmt::Display for ProxyCoreError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io { context, source } => write!(formatter, "{}: {}", context, source),
            Self::StartupFailed(message) => write!(formatter, "{}", message),
        }
    }
}

impl std::error::Error for ProxyCoreError {}

#[derive(Debug)]
pub enum ManagementCoreError {
    Unavailable(String),
    Api(ManagementApiError),
}

impl std::fmt::Display for ManagementCoreError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unavailable(message) => write!(formatter, "{}", message),
            Self::Api(error) => write!(formatter, "{}", error),
        }
    }
}

impl std::error::Error for ManagementCoreError {}

impl From<ManagementApiError> for ManagementCoreError {
    fn from(error: ManagementApiError) -> Self {
        Self::Api(error)
    }
}

fn io_error(context: &'static str, source: std::io::Error) -> ProxyCoreError {
    ProxyCoreError::Io { context, source }
}

pub struct AppCore {
    settings: AppSettings,
    proxy: ProxyLifecycle,
    management_snapshot: ManagementSnapshot,
    fallback: FallbackConfiguration,
    fallback_runtime: FallbackRuntimeState,
    quotas: Vec<AccountQuota>,
    request_logs: Vec<RequestLogEntry>,
    credential_error: Option<String>,
}

impl Default for AppCore {
    fn default() -> Self {
        let mut settings = read_settings().unwrap_or_default();
        let mut credential_error = migrate_remote_management_key(&mut settings);
        let (management_key, local_credential_error) = load_or_create_local_management_key();
        if credential_error.is_none() {
            credential_error = local_credential_error;
        } else if let Some(local_error) = local_credential_error {
            credential_error = credential_error.map(|error| format!("{} {}", error, local_error));
        }
        if let Err(error) = write_settings(&settings) {
            let message = format!("无法保存应用设置：{}", error);
            credential_error = Some(match credential_error {
                Some(existing) => format!("{} {}", existing, message),
                None => message,
            });
        }
        Self {
            proxy: ProxyLifecycle::new(&settings, management_key),
            settings,
            management_snapshot: ManagementSnapshot::default(),
            fallback: read_fallback_configuration().unwrap_or_default(),
            fallback_runtime: FallbackRuntimeState::default(),
            quotas: Vec::new(),
            request_logs: Vec::new(),
            credential_error,
        }
    }
}

impl AppCore {
    pub fn app_state(&mut self) -> AppState {
        self.proxy.refresh(&self.settings);
        self.state_snapshot()
    }

    pub fn platform_info(&self) -> PlatformInfo {
        quotio_platform::platform_info()
    }

    pub fn save_settings(
        &mut self,
        mut settings: AppSettings,
    ) -> Result<AppState, ManagementCoreError> {
        if let Some(remote_key) = settings
            .remote_management_key
            .as_deref()
            .map(str::trim)
            .filter(|key| !key.is_empty())
            .map(ToOwned::to_owned)
        {
            match quotio_platform::set_credential(
                quotio_platform::REMOTE_MANAGEMENT_KEY_ACCOUNT,
                &remote_key,
            ) {
                Ok(()) => {
                    settings.remote_management_key = None;
                    self.credential_error = None;
                }
                Err(error) => {
                    self.credential_error =
                        Some(format!("远程管理密钥无法写入安全存储：{}", error));
                    settings.remote_management_key = Some(remote_key);
                }
            }
        }

        let mut persisted = settings.clone();
        persisted.remote_management_key = None;
        write_settings(&persisted).map_err(|error| {
            ManagementCoreError::Unavailable(format!("无法保存应用设置：{}", error))
        })?;

        self.settings = settings;
        if self.settings.remote_management_key.is_none() {
            self.settings.remote_management_key = None;
        }
        self.proxy.sync_settings(&self.settings);
        // Keep config.yaml in sync with settings immediately (not only on proxy
        // start) so changes persist into CLIProxyAPI's config + a running proxy
        // can pick them up on its next reload.
        let _ = self.proxy.write_config(&self.settings);
        Ok(self.app_state())
    }

    pub fn start_proxy(&mut self) -> Result<AppState, ProxyCoreError> {
        self.proxy.start(&self.settings)?;
        Ok(self.app_state())
    }

    pub fn stop_proxy(&mut self) -> Result<AppState, ProxyCoreError> {
        self.proxy.stop(&self.settings)?;
        Ok(self.app_state())
    }

    pub fn restart_proxy(&mut self) -> Result<AppState, ProxyCoreError> {
        self.proxy.stop(&self.settings)?;
        thread::sleep(Duration::from_millis(250));
        self.proxy.start(&self.settings)?;
        Ok(self.app_state())
    }

    pub fn proxy_managed_binary_path(&self) -> PathBuf {
        self.proxy.managed_binary_path()
    }

    /// Path where the managed cloudflared binary lives (next to the proxy core).
    pub fn cloudflared_binary_path(&self) -> PathBuf {
        self.proxy_managed_binary_path()
            .parent()
            .map(|dir| dir.join("cloudflared.exe"))
            .unwrap_or_else(|| PathBuf::from("cloudflared.exe"))
    }

    /// The local port the proxy listens on (used by the tunnel target URL).
    pub fn proxy_port(&self) -> u16 {
        self.settings.proxy_port
    }

    pub fn finalize_proxy_download(&mut self, tag: String) -> AppState {
        self.proxy.finalize_download(tag, &self.settings);
        self.app_state()
    }

    pub fn check_proxy_health(&mut self) -> AppState {
        self.proxy.check_health(&self.settings);
        self.app_state()
    }

    pub fn management_client(&mut self) -> Result<ManagementApiClient, ManagementCoreError> {
        self.proxy.refresh(&self.settings);

        if matches!(self.settings.connection_mode, ConnectionMode::Local)
            && !matches!(self.proxy.state.status, ProxyStatusKind::Running)
        {
            return Err(ManagementCoreError::Unavailable(
                "代理核心未运行，无法访问本地管理接口。".to_string(),
            ));
        }

        let management_key = match self.settings.connection_mode {
            ConnectionMode::Local => self.proxy.management_key.clone(),
            ConnectionMode::Remote => self
                .settings
                .remote_management_key
                .as_deref()
                .map(str::trim)
                .filter(|key| !key.is_empty())
                .map(ToOwned::to_owned)
                .or_else(secure_remote_management_key)
                .ok_or_else(|| {
                    ManagementCoreError::Unavailable("远程管理接口密钥未配置。".to_string())
                })?,
        };

        Ok(ManagementApiClient::local(
            self.proxy.state.management_endpoint.clone(),
            management_key,
        ))
    }

    pub async fn refresh_management_snapshot(&mut self) -> Result<AppState, ManagementCoreError> {
        let client = self.management_client()?;
        // Enable per-request telemetry so the usage queue fills (idempotent).
        let _ = client.set_usage_statistics_enabled(true).await;
        let snapshot = client.refresh_snapshot().await?;
        let request_logs = client.fetch_request_logs(200).await.unwrap_or_default();
        self.ingest_request_logs(request_logs);
        Ok(self.apply_management_snapshot(snapshot))
    }

    /// Accumulate freshly-drained request records (newest first), capped.
    pub fn ingest_request_logs(&mut self, mut records: Vec<RequestLogEntry>) {
        if records.is_empty() {
            return;
        }
        records.reverse();
        records.append(&mut self.request_logs);
        records.truncate(500);
        self.request_logs = records;
    }

    pub fn apply_management_snapshot(&mut self, snapshot: ManagementSnapshot) -> AppState {
        self.management_snapshot = snapshot;
        self.app_state()
    }

    /// Fetch real provider quotas (Codex/OpenAI today) and cache them.
    /// Network I/O is blocking, so callers should invoke this off the hot path
    /// (e.g. the Quota page refresh action), not on every `app_state()`.
    pub fn refresh_quotas(&mut self) -> AppState {
        self.quotas = quota::fetch_all_quotas(self.proxy_upstream_url().as_deref());
        self.app_state()
    }

    /// The upstream proxy URL the user configured in Settings, if non-empty.
    /// Quota fetching routes provider requests through it (mirroring the macOS
    /// reference app), falling back to OS proxy env vars when it is empty.
    pub fn proxy_upstream_url(&self) -> Option<String> {
        let trimmed = self.settings.proxy_url.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    }

    /// Store quotas fetched off-thread, so the Tauri command can run the
    /// (blocking) network fetch without holding the lock or blocking the UI.
    pub fn set_quotas(&mut self, quotas: Vec<AccountQuota>) -> AppState {
        self.quotas = quotas;
        self.app_state()
    }

    /// Import a CLIProxyAPI account JSON file into the auth directory
    /// (`~/.cli-proxy-api`) so its account shows up in quota and is usable by
    /// the proxy. The filename is sanitized to a basename.
    pub fn import_auth_file(&mut self, filename: &str, content: &str) -> Result<AppState, String> {
        let base = std::path::Path::new(filename)
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| "无效的文件名".to_string())?;
        if !base.to_ascii_lowercase().ends_with(".json") {
            return Err("仅支持 .json 账号文件".to_string());
        }
        serde_json::from_str::<serde_json::Value>(content)
            .map_err(|_| "文件内容不是有效的 JSON".to_string())?;
        let dir = quotio_platform::proxy_auth_dir();
        std::fs::create_dir_all(&dir).map_err(|error| format!("创建 auth 目录失败：{}", error))?;
        std::fs::write(dir.join(base), content)
            .map_err(|error| format!("写入账号文件失败：{}", error))?;
        Ok(self.app_state())
    }

    pub fn update_fallback_configuration(
        &mut self,
        action: FallbackConfigAction,
    ) -> Result<AppState, ManagementCoreError> {
        self.fallback = apply_fallback_action(self.fallback.clone(), action)?;
        self.fallback_runtime.route_states = derive_fallback_route_states(&self.fallback);
        if !self.fallback.is_route_caching_enabled || !self.fallback.is_enabled {
            self.fallback_runtime.route_states.clear();
        }
        write_fallback_configuration(&self.fallback).map_err(|error| {
            ManagementCoreError::Unavailable(format!("无法保存 fallback 配置：{}", error))
        })?;
        Ok(self.app_state())
    }

    pub fn detect_agents(&mut self) -> AppState {
        self.app_state()
    }

    pub fn read_agent_configuration(
        &self,
        agent_id: &str,
    ) -> Result<SavedAgentConfiguration, ManagementCoreError> {
        agent_config::read_agent_configuration(agent_id)
    }

    pub fn configure_agent(
        &mut self,
        request: AgentConfigurationRequest,
    ) -> Result<AppState, ManagementCoreError> {
        agent_config::configure_agent(request)?;
        Ok(self.app_state())
    }

    pub fn configure_agent_with_result(
        &mut self,
        request: AgentConfigurationRequest,
    ) -> Result<AgentConfigurationResult, ManagementCoreError> {
        agent_config::configure_agent(request)
    }

    pub fn list_agent_backups(
        &self,
        agent_id: &str,
    ) -> Result<Vec<AgentBackupFile>, ManagementCoreError> {
        agent_config::list_agent_backups(agent_id)
    }

    pub fn restore_agent_backup(
        &mut self,
        agent_id: &str,
        backup_path: &str,
    ) -> Result<AgentConfigurationResult, ManagementCoreError> {
        agent_config::restore_agent_backup(agent_id, backup_path)
    }

    pub fn reset_agent_configuration(
        &mut self,
        agent_id: &str,
    ) -> Result<AgentConfigurationResult, ManagementCoreError> {
        agent_config::reset_agent_configuration(agent_id)
    }

    pub fn credential_status(&self) -> CredentialStatus {
        credential_status(self.credential_error.as_deref())
    }

    pub fn clear_remote_management_key(&mut self) -> Result<AppState, ManagementCoreError> {
        quotio_platform::delete_credential(quotio_platform::REMOTE_MANAGEMENT_KEY_ACCOUNT)
            .map_err(|error| {
                ManagementCoreError::Unavailable(format!("无法删除远程管理密钥：{}", error))
            })?;
        self.settings.remote_management_key = None;
        Ok(self.app_state())
    }

    pub fn discover_available_models(&mut self) -> AppState {
        self.fallback_runtime.available_models =
            models_from_management_snapshot(&self.management_snapshot);
        self.fallback_runtime.model_discovery_status =
            if self.fallback_runtime.available_models.is_empty() {
                self.fallback_runtime.available_models = default_available_models();
                "using_builtin_defaults".to_string()
            } else {
                "from_management_snapshot".to_string()
            };
        self.app_state()
    }

    pub fn refresh_fallback_route_state(&mut self) -> AppState {
        self.fallback_runtime.route_states = derive_fallback_route_states(&self.fallback);
        self.app_state()
    }

    pub fn open_config_root(&self) -> Result<(), ManagementCoreError> {
        quotio_platform::open_file_manager(&quotio_platform::app_config_dir()).map_err(|error| {
            ManagementCoreError::Unavailable(format!("无法打开配置目录：{}", error))
        })
    }

    fn state_snapshot(&self) -> AppState {
        let mut settings = self.settings.clone();
        settings.remote_management_key = None;

        // When the proxy management API reports no auth files (proxy not
        // connected, or stats disabled), backfill from the local auth dir so
        // accounts still appear on the dashboard / providers screens.
        let mut management = self.management_snapshot.clone();
        if management.auth_files.is_empty() {
            management.auth_files = list_local_accounts();
        }
        let auth_files = management.auth_files.clone();

        AppState {
            migration_phase: MigrationPhase::PlatformAdapters,
            platform: self.platform_info(),
            settings,
            proxy: self.proxy.state.clone(),
            proxy_resources: proxy_resource_status(&self.proxy.paths),
            providers: default_providers(),
            management,
            auth_files,
            quotas: self.quotas.clone(),
            logs: self.request_logs.clone(),
            agents: agents::detect_agents(),
            api_keys: api_key_entries(&get_api_keys()),
            request_stats: request_stats_from_management(&self.management_snapshot),
            fallback: self.fallback.clone(),
            fallback_runtime: self.fallback_runtime.clone(),
            credentials: self.credential_status(),
            platform_features: quotio_platform::platform_feature_state(
                self.settings.launch_at_login,
                self.settings.notifications_enabled,
            ),
            config_root: quotio_platform::app_config_dir().display().to_string(),
        }
    }
}

fn load_or_create_local_api_key() -> String {
    match quotio_platform::get_credential(quotio_platform::LOCAL_API_KEY_ACCOUNT) {
        Ok(Some(key)) if !key.trim().is_empty() => key,
        _ => {
            let key = format!("quotio-local-{}", Uuid::new_v4());
            let _ = quotio_platform::set_credential(quotio_platform::LOCAL_API_KEY_ACCOUNT, &key);
            key
        }
    }
}

fn api_keys_path() -> PathBuf {
    quotio_platform::app_config_dir().join("api-keys.json")
}

/// The persisted proxy api-keys (client auth keys written into config.yaml).
/// Seeded once with the stable local key; survives restarts until the user
/// edits the list. This is the source of truth, not the regenerated config.yaml.
pub fn get_api_keys() -> Vec<String> {
    match std::fs::read_to_string(api_keys_path())
        .ok()
        .and_then(|raw| serde_json::from_str::<Vec<String>>(&raw).ok())
    {
        Some(keys) => keys,
        None => {
            let seed = vec![load_or_create_local_api_key()];
            let _ = save_api_keys(&seed);
            seed
        }
    }
}

fn save_api_keys(keys: &[String]) -> Result<(), String> {
    let path = api_keys_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let body = serde_json::to_string_pretty(keys).map_err(|error| error.to_string())?;
    std::fs::write(&path, body).map_err(|error| error.to_string())
}

pub fn add_api_key(key: String) -> Result<Vec<String>, String> {
    let key = key.trim().to_string();
    if key.is_empty() {
        return Err("API 密钥不能为空。".to_string());
    }
    let mut keys = get_api_keys();
    if !keys.contains(&key) {
        keys.push(key);
        save_api_keys(&keys)?;
    }
    Ok(keys)
}

pub fn remove_api_key(key: &str) -> Result<Vec<String>, String> {
    let mut keys = get_api_keys();
    keys.retain(|existing| existing != key);
    save_api_keys(&keys)?;
    Ok(keys)
}

pub fn update_api_key(old: &str, replacement: String) -> Result<Vec<String>, String> {
    let replacement = replacement.trim().to_string();
    if replacement.is_empty() {
        return Err("API 密钥不能为空。".to_string());
    }
    let mut keys = get_api_keys();
    let mut replaced = false;
    for existing in keys.iter_mut() {
        if existing == old {
            *existing = replacement.clone();
            replaced = true;
        }
    }
    if !replaced && !keys.contains(&replacement) {
        keys.push(replacement);
    }
    save_api_keys(&keys)?;
    Ok(keys)
}

fn load_or_create_local_management_key() -> (String, Option<String>) {
    match quotio_platform::get_credential(quotio_platform::LOCAL_MANAGEMENT_KEY_ACCOUNT) {
        Ok(Some(key)) if !key.trim().is_empty() => (key, None),
        Ok(_) => {
            let key = format!("quotio-management-{}", Uuid::new_v4());
            match quotio_platform::set_credential(
                quotio_platform::LOCAL_MANAGEMENT_KEY_ACCOUNT,
                &key,
            ) {
                Ok(()) => (key, None),
                Err(error) => (
                    key,
                    Some(format!("本地管理密钥无法写入安全存储：{}", error)),
                ),
            }
        }
        Err(error) => (
            format!("quotio-management-{}", Uuid::new_v4()),
            Some(format!("本地管理密钥无法从安全存储读取：{}", error)),
        ),
    }
}

fn secure_remote_management_key() -> Option<String> {
    quotio_platform::get_credential(quotio_platform::REMOTE_MANAGEMENT_KEY_ACCOUNT)
        .ok()
        .flatten()
        .filter(|key| !key.trim().is_empty())
}

fn credential_status(error: Option<&str>) -> CredentialStatus {
    let availability = quotio_platform::credential_availability();
    let local_key = quotio_platform::get_credential(quotio_platform::LOCAL_MANAGEMENT_KEY_ACCOUNT)
        .ok()
        .flatten();
    let remote_key = secure_remote_management_key();
    let mut message = quotio_platform::credential_status_message(&availability);
    if let Some(error) = error {
        message = format!("{} {}", message, error);
    }

    CredentialStatus {
        availability,
        local_management_key_configured: local_key
            .as_deref()
            .is_some_and(|key| !key.trim().is_empty()),
        remote_management_key_configured: remote_key
            .as_deref()
            .is_some_and(|key| !key.trim().is_empty()),
        remote_management_key_masked: remote_key.as_deref().map(mask_secret),
        message,
    }
}

fn models_from_management_snapshot(_snapshot: &ManagementSnapshot) -> Vec<AvailableModel> {
    Vec::new()
}

fn derive_fallback_route_states(config: &FallbackConfiguration) -> Vec<FallbackRouteState> {
    if !config.is_enabled || !config.is_route_caching_enabled {
        return Vec::new();
    }

    config
        .virtual_models
        .iter()
        .filter(|model| model.is_enabled)
        .filter_map(|model| {
            let mut entries = model.fallback_entries.clone();
            entries.sort_by_key(|entry| entry.priority);
            let current_entry = entries.first()?.clone();
            Some(FallbackRouteState {
                virtual_model_name: model.name.clone(),
                current_entry_index: 0,
                current_entry,
                last_updated_unix_seconds: quotio_platform::current_unix_seconds(),
                total_entries: entries.len(),
            })
        })
        .collect()
}

fn api_key_entries(keys: &[String]) -> Vec<ApiKeyEntry> {
    keys.iter()
        .map(|key| {
            ApiKeyEntry {
                value: key.clone(),
                masked_value: mask_secret(key),
                source: "management".to_string(),
            }
        })
        .collect()
}

fn request_stats_from_management(snapshot: &ManagementSnapshot) -> Option<RequestStats> {
    let stats = snapshot.usage.as_ref()?;
    let total_requests = stats
        .usage
        .as_ref()
        .and_then(|usage| usage.total_requests)
        .unwrap_or_default();
    let successful_requests = stats
        .usage
        .as_ref()
        .and_then(|usage| usage.success_count)
        .unwrap_or_default();
    let failed_requests = stats
        .failed_requests
        .or_else(|| stats.usage.as_ref().and_then(|usage| usage.failure_count))
        .unwrap_or_default();
    let total_input_tokens = stats
        .usage
        .as_ref()
        .and_then(|usage| usage.input_tokens)
        .unwrap_or_default();
    let total_output_tokens = stats
        .usage
        .as_ref()
        .and_then(|usage| usage.output_tokens)
        .unwrap_or_default();

    Some(RequestStats {
        total_requests,
        successful_requests,
        failed_requests,
        total_input_tokens,
        total_output_tokens,
        average_duration_ms: 0,
    })
}

fn apply_fallback_action(
    mut config: FallbackConfiguration,
    action: FallbackConfigAction,
) -> Result<FallbackConfiguration, ManagementCoreError> {
    match action {
        FallbackConfigAction::SetEnabled { enabled } => config.is_enabled = enabled,
        FallbackConfigAction::SetRouteCaching { enabled } => {
            config.is_route_caching_enabled = enabled;
        }
        FallbackConfigAction::AddVirtualModel { name } => {
            let name = normalized_required_name(&name, "虚拟模型名称不能为空。")?;
            ensure_unique_virtual_model_name(&config, &name, None)?;
            config.virtual_models.push(VirtualModel {
                id: Uuid::new_v4().to_string(),
                name,
                fallback_entries: Vec::new(),
                is_enabled: true,
            });
        }
        FallbackConfigAction::RenameVirtualModel { id, name } => {
            let name = normalized_required_name(&name, "虚拟模型名称不能为空。")?;
            ensure_unique_virtual_model_name(&config, &name, Some(&id))?;
            let model = find_virtual_model_mut(&mut config, &id)?;
            model.name = name;
        }
        FallbackConfigAction::RemoveVirtualModel { id } => {
            config.virtual_models.retain(|model| model.id != id);
        }
        FallbackConfigAction::ToggleVirtualModel { id, enabled } => {
            let model = find_virtual_model_mut(&mut config, &id)?;
            model.is_enabled = enabled;
        }
        FallbackConfigAction::AddEntry {
            virtual_model_id,
            provider_id,
            model_id,
        } => {
            let provider_id = normalized_required_name(&provider_id, "provider 不能为空。")?;
            let model_id = normalized_required_name(&model_id, "model 不能为空。")?;
            let model = find_virtual_model_mut(&mut config, &virtual_model_id)?;
            let priority = model
                .fallback_entries
                .iter()
                .map(|entry| entry.priority)
                .max()
                .unwrap_or(0)
                .saturating_add(1);
            model.fallback_entries.push(FallbackEntry {
                id: Uuid::new_v4().to_string(),
                provider_id,
                model_id,
                priority,
            });
        }
        FallbackConfigAction::RemoveEntry {
            virtual_model_id,
            entry_id,
        } => {
            let model = find_virtual_model_mut(&mut config, &virtual_model_id)?;
            model.fallback_entries.retain(|entry| entry.id != entry_id);
            normalize_entry_priorities(model);
        }
        FallbackConfigAction::MoveEntry {
            virtual_model_id,
            entry_id,
            direction,
        } => {
            let model = find_virtual_model_mut(&mut config, &virtual_model_id)?;
            model.fallback_entries.sort_by_key(|entry| entry.priority);
            let Some(index) = model
                .fallback_entries
                .iter()
                .position(|entry| entry.id == entry_id)
            else {
                return Err(ManagementCoreError::Unavailable(
                    "fallback entry 不存在。".to_string(),
                ));
            };
            match direction {
                FallbackEntryMoveDirection::Up if index > 0 => {
                    model.fallback_entries.swap(index, index - 1);
                }
                FallbackEntryMoveDirection::Down if index + 1 < model.fallback_entries.len() => {
                    model.fallback_entries.swap(index, index + 1);
                }
                _ => {}
            }
            normalize_entry_priorities(model);
        }
        FallbackConfigAction::Reset => config = FallbackConfiguration::default(),
    }

    Ok(config)
}

fn normalized_required_name(
    value: &str,
    empty_message: &str,
) -> Result<String, ManagementCoreError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(ManagementCoreError::Unavailable(empty_message.to_string()));
    }
    Ok(trimmed.to_string())
}

fn ensure_unique_virtual_model_name(
    config: &FallbackConfiguration,
    name: &str,
    current_id: Option<&str>,
) -> Result<(), ManagementCoreError> {
    let is_duplicate = config.virtual_models.iter().any(|model| {
        current_id != Some(model.id.as_str()) && model.name.eq_ignore_ascii_case(name)
    });
    if is_duplicate {
        return Err(ManagementCoreError::Unavailable(
            "虚拟模型名称已存在。".to_string(),
        ));
    }
    Ok(())
}

fn find_virtual_model_mut<'a>(
    config: &'a mut FallbackConfiguration,
    id: &str,
) -> Result<&'a mut VirtualModel, ManagementCoreError> {
    config
        .virtual_models
        .iter_mut()
        .find(|model| model.id == id)
        .ok_or_else(|| ManagementCoreError::Unavailable("虚拟模型不存在。".to_string()))
}

fn normalize_entry_priorities(model: &mut VirtualModel) {
    model.fallback_entries.sort_by_key(|entry| entry.priority);
    for (index, entry) in model.fallback_entries.iter_mut().enumerate() {
        entry.priority = u16::try_from(index + 1).unwrap_or(u16::MAX);
    }
}

fn fallback_config_path() -> PathBuf {
    quotio_platform::app_config_dir().join("fallback.json")
}

/// Port the ProxyBridge listens on (proxy port + 100) when fallback is enabled.
fn fallback_port(proxy_port: u16) -> u16 {
    if proxy_port < 65_435 {
        proxy_port + 100
    } else {
        proxy_port - 100
    }
}

/// List CLIProxyAPI account files in the local auth dir as AuthFile entries, so
/// the Providers page can show existing accounts even when the proxy isn't
/// connected (the proxy's /auth-files is empty then).
pub fn list_local_accounts() -> Vec<AuthFile> {
    let dir = quotio_platform::proxy_auth_dir();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut files = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        let lower = name.to_ascii_lowercase();
        if !lower.ends_with(".json") || lower.starts_with("glm-keys") {
            continue;
        }
        let parsed = std::fs::read_to_string(&path)
            .ok()
            .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok());
        // Prefer the account file's own "type"/"provider" so non-standard
        // filenames still group correctly; fall back to the filename prefix.
        let provider = parsed
            .as_ref()
            .and_then(|value| {
                value
                    .get("type")
                    .or_else(|| value.get("provider"))
                    .and_then(|kind| kind.as_str())
            })
            .filter(|kind| !kind.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| name.split('-').next().unwrap_or("").to_string());
        let email = parsed.as_ref().and_then(|value| {
            value
                .get("email")
                .and_then(|email| email.as_str())
                .map(str::to_string)
        });
        files.push(AuthFile {
            id: name.to_string(),
            name: name.to_string(),
            provider,
            label: None,
            status: "local".to_string(),
            status_message: None,
            disabled: false,
            unavailable: false,
            runtime_only: Some(false),
            source: Some("local".to_string()),
            path: Some(path.display().to_string()),
            email,
            account_type: None,
            account: None,
            auth_index: None,
            created_at: None,
            updated_at: None,
            last_refresh: None,
        });
    }
    files.sort_by(|left, right| left.name.cmp(&right.name));
    files
}

/// A user-defined third-party provider (OpenAI/Gemini-compatible endpoint).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CustomProvider {
    pub id: String,
    pub name: String,
    pub base_url: String,
    pub api_key: String,
    pub kind: String,
    #[serde(default)]
    pub prefix: String,
}

fn custom_providers_path() -> PathBuf {
    quotio_platform::app_config_dir().join("custom-providers.json")
}

pub fn list_custom_providers() -> Vec<CustomProvider> {
    std::fs::read_to_string(custom_providers_path())
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

fn save_custom_providers(list: &[CustomProvider]) -> Result<(), String> {
    let path = custom_providers_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| format!("创建配置目录失败：{}", error))?;
    }
    let json = serde_json::to_string_pretty(list).map_err(|error| format!("序列化失败：{}", error))?;
    std::fs::write(&path, json).map_err(|error| format!("写入自定义服务商失败：{}", error))
}

pub fn add_custom_provider(
    name: String,
    base_url: String,
    api_key: String,
    kind: String,
    prefix: String,
) -> Result<Vec<CustomProvider>, String> {
    let name = name.trim();
    let base_url = base_url.trim();
    if name.is_empty() || base_url.is_empty() {
        return Err("名称和 Base URL 必填。".to_string());
    }
    let kind = kind.trim();
    let mut list = list_custom_providers();
    list.push(CustomProvider {
        id: Uuid::new_v4().to_string(),
        name: name.to_string(),
        base_url: base_url.to_string(),
        api_key: api_key.trim().to_string(),
        kind: if kind.is_empty() { "openai".to_string() } else { kind.to_string() },
        prefix: prefix.trim().to_string(),
    });
    save_custom_providers(&list)?;
    Ok(list)
}

pub fn delete_custom_provider(id: &str) -> Result<Vec<CustomProvider>, String> {
    let mut list = list_custom_providers();
    list.retain(|provider| provider.id != id);
    save_custom_providers(&list)?;
    Ok(list)
}

pub fn update_custom_provider(
    id: String,
    name: String,
    base_url: String,
    api_key: String,
    kind: String,
    prefix: String,
) -> Result<Vec<CustomProvider>, String> {
    let name = name.trim();
    let base_url = base_url.trim();
    if name.is_empty() || base_url.is_empty() {
        return Err("名称和 Base URL 必填。".to_string());
    }
    let kind = kind.trim();
    let mut list = list_custom_providers();
    let Some(provider) = list.iter_mut().find(|provider| provider.id == id) else {
        return Err("未找到要编辑的服务商。".to_string());
    };
    provider.name = name.to_string();
    provider.base_url = base_url.to_string();
    provider.api_key = api_key.trim().to_string();
    provider.kind = if kind.is_empty() {
        "openai".to_string()
    } else {
        kind.to_string()
    };
    provider.prefix = prefix.trim().to_string();
    save_custom_providers(&list)?;
    Ok(list)
}

/// Render custom providers as CLIProxyAPI config.yaml sections so the proxy
/// loads + routes to them on start.
fn custom_providers_yaml() -> String {
    let providers = list_custom_providers();
    if providers.is_empty() {
        return String::new();
    }
    let mut by_type: std::collections::BTreeMap<String, Vec<&CustomProvider>> =
        std::collections::BTreeMap::new();
    for provider in &providers {
        by_type
            .entry(custom_provider_yaml_type(&provider.kind))
            .or_default()
            .push(provider);
    }
    let mut out = String::from("\n# Custom Providers (managed by Quotio)\n");
    for (section, list) in &by_type {
        out.push_str(section);
        out.push_str(":\n");
        for provider in list {
            let name = provider.name.replace('"', "'");
            let base_url = provider.base_url.replace('"', "'");
            let api_key = provider.api_key.replace('"', "'");
            let prefix = provider.prefix.replace('"', "'");
            if section == "openai-compatibility" {
                out.push_str(&format!("  - name: \"{}\"\n", name));
                out.push_str(&format!("    base-url: \"{}\"\n", base_url));
                if !prefix.is_empty() {
                    out.push_str(&format!("    prefix: \"{}\"\n", prefix));
                }
                if !api_key.is_empty() {
                    out.push_str("    api-key-entries:\n");
                    out.push_str(&format!("      - api-key: \"{}\"\n", api_key));
                }
            } else {
                out.push_str(&format!("  - api-key: \"{}\"\n", api_key));
                if !base_url.is_empty() {
                    out.push_str(&format!("    base-url: \"{}\"\n", base_url));
                }
                if !prefix.is_empty() {
                    out.push_str(&format!("    prefix: \"{}\"\n", prefix));
                }
            }
        }
    }
    out
}

fn custom_provider_yaml_type(kind: &str) -> String {
    match kind {
        "openai" => "openai-compatibility",
        "gemini" => "gemini-api-key",
        "claude" => "claude-api-key",
        "codex" => "codex-api-key",
        "glm" => "glm-api-key",
        other => other,
    }
    .to_string()
}

fn settings_path() -> PathBuf {
    quotio_platform::app_config_dir().join("settings.json")
}

fn read_settings() -> Option<AppSettings> {
    let path = settings_path();
    let content = fs::read_to_string(path).ok()?;
    serde_json::from_str(&content).ok()
}

fn write_settings(settings: &AppSettings) -> std::io::Result<()> {
    let path = settings_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut persisted = settings.clone();
    persisted.remote_management_key = None;
    let content = serde_json::to_string_pretty(&persisted).unwrap_or_else(|_| "{}".to_string());
    fs::write(path, content)
}

fn migrate_remote_management_key(settings: &mut AppSettings) -> Option<String> {
    let Some(remote_key) = settings
        .remote_management_key
        .as_deref()
        .map(str::trim)
        .filter(|key| !key.is_empty())
        .map(ToOwned::to_owned)
    else {
        return None;
    };

    match quotio_platform::set_credential(
        quotio_platform::REMOTE_MANAGEMENT_KEY_ACCOUNT,
        &remote_key,
    ) {
        Ok(()) => {
            settings.remote_management_key = None;
            None
        }
        Err(error) => Some(format!("远程管理密钥无法迁入安全存储：{}", error)),
    }
}

fn read_fallback_configuration() -> Option<FallbackConfiguration> {
    let path = fallback_config_path();
    let content = fs::read_to_string(path).ok()?;
    serde_json::from_str(&content).ok()
}

fn write_fallback_configuration(config: &FallbackConfiguration) -> std::io::Result<()> {
    let path = fallback_config_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let content = serde_json::to_string_pretty(config).unwrap_or_else(|_| "{}".to_string());
    fs::write(path, content)
}

struct ProxyLifecycle {
    child: Option<Child>,
    state: ProxyState,
    paths: ProxyPaths,
    management_key: String,
    local_api_key: String,
    crash_count: u32,
    bridge: Option<crate::bridge::ProxyBridge>,
}

impl ProxyLifecycle {
    fn new(settings: &AppSettings, management_key: String) -> Self {
        let paths = ProxyPaths::new();
        let has_binary =
            paths.resolve_resource_binary().is_some() || paths.managed_binary_path().is_file();
        let state = if has_binary {
            state_for_paths(
                settings,
                &paths,
                ProxyStatusKind::Stopped,
                None,
                None,
                0,
                ProxyHealthState::default(),
                "代理核心尚未启动。",
            )
        } else {
            missing_binary_state(settings, &paths, 0)
        };
        Self {
            child: None,
            state,
            paths,
            management_key,
            local_api_key: load_or_create_local_api_key(),
            crash_count: 0,
            bridge: None,
        }
    }

    fn sync_settings(&mut self, settings: &AppSettings) {
        let status = self.state.status.clone();
        let pid = self.child.as_ref().map(Child::id);
        let exit_code = self.state.exit_code;
        let health = self.state.health.clone();
        let message = self.state.message.clone();
        self.state = state_for_paths(
            settings,
            &self.paths,
            status,
            pid,
            exit_code,
            self.crash_count,
            health,
            message,
        );
    }

    fn start(&mut self, settings: &AppSettings) -> Result<(), ProxyCoreError> {
        self.refresh(settings);

        if self.child.is_some() {
            self.state = state_for_paths(
                settings,
                &self.paths,
                ProxyStatusKind::Running,
                self.child.as_ref().map(Child::id),
                None,
                self.crash_count,
                self.state.health.clone(),
                "代理核心已经在运行。",
            );
            return Ok(());
        }

        let resource_binary = self.paths.resolve_resource_binary();
        let managed_existing = self.paths.managed_binary_path();
        if resource_binary.is_none() && !managed_existing.is_file() {
            self.state = missing_binary_state(settings, &self.paths, self.crash_count);
            return Ok(());
        }

        self.state = state_for_paths(
            settings,
            &self.paths,
            ProxyStatusKind::Starting,
            None,
            None,
            self.crash_count,
            ProxyHealthState::unknown("代理进程正在启动。"),
            "正在启动代理核心。",
        );

        // Prefer a bundled resource binary (copy it into the managed dir);
        // otherwise reuse a previously downloaded managed binary.
        let managed_binary = match resource_binary {
            Some(resource) => self
                .paths
                .prepare_managed_binary(&resource)
                .map_err(|error| io_error("无法准备代理二进制", error))?,
            None => managed_existing,
        };

        self.write_config(settings)
            .map_err(|error| io_error("无法写入代理配置", error))?;

        let mut command = Command::new(&managed_binary);
        command
            .arg("-config")
            .arg(&self.paths.config_path)
            .current_dir(managed_binary.parent().unwrap_or_else(|| Path::new(".")))
            .env("TERM", "xterm-256color")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());

        // Don't pop up a console window for the proxy core on Windows.
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            command.creation_flags(CREATE_NO_WINDOW);
        }

        let mut child = command
            .spawn()
            .map_err(|error| io_error("无法启动代理进程", error))?;
        let pid = child.id();

        thread::sleep(Duration::from_millis(900));

        match child
            .try_wait()
            .map_err(|error| io_error("无法读取代理进程状态", error))?
        {
            Some(exit_status) => {
                let code = exit_status.code().unwrap_or(-1);
                self.child = None;
                self.crash_count = self.crash_count.saturating_add(1);
                self.state = state_for_paths(
                    settings,
                    &self.paths,
                    ProxyStatusKind::Crashed,
                    None,
                    Some(code),
                    self.crash_count,
                    ProxyHealthState::unhealthy(now_unix_seconds(), "代理进程启动后立即退出。"),
                    format!("代理进程启动失败，退出码：{}。", code),
                );
                return Err(ProxyCoreError::StartupFailed(format!(
                    "代理进程启动失败，退出码：{}。",
                    code
                )));
            }
            None => {
                self.child = Some(child);
                let health = self.probe_health(settings);
                let message = if health.ok == Some(true) {
                    "代理核心已启动并通过健康检查。"
                } else {
                    "代理核心已启动，但健康检查尚未通过。"
                };
                self.state = state_for_paths(
                    settings,
                    &self.paths,
                    ProxyStatusKind::Running,
                    Some(pid),
                    None,
                    self.crash_count,
                    health,
                    message,
                );
            }
        }

        // When fallback is enabled, run the ProxyBridge in front of the proxy on
        // a separate port (proxy_port + 100) so virtual-model requests get
        // resolved + retried. Default (fallback off) leaves the proxy untouched.
        self.bridge = None;
        if read_fallback_configuration()
            .map(|config| config.is_enabled)
            .unwrap_or(false)
        {
            self.bridge = crate::bridge::ProxyBridge::start(
                fallback_port(settings.proxy_port),
                settings.proxy_port,
                fallback_config_path(),
            )
            .ok();
        }

        Ok(())
    }

    /// Path where the managed proxy binary lives. The actual download happens
    /// off the core lock (so the UI never freezes), then `finalize_download`
    /// updates the state.
    fn managed_binary_path(&self) -> PathBuf {
        self.paths.managed_binary_path()
    }

    /// Mark the binary as downloaded + ready (called after the off-thread
    /// download completes).
    fn finalize_download(&mut self, tag: String, settings: &AppSettings) {
        let dest = self.paths.managed_binary_path();
        let _ = make_executable(&dest);
        let message = if tag.is_empty() {
            "代理核心已下载，可以启动。".to_string()
        } else {
            format!("代理核心 {} 已下载，可以启动。", tag)
        };
        self.state = state_for_paths(
            settings,
            &self.paths,
            ProxyStatusKind::Stopped,
            None,
            None,
            self.crash_count,
            ProxyHealthState::unknown("代理核心已就绪。"),
            message,
        );
    }

    fn stop(&mut self, settings: &AppSettings) -> Result<(), ProxyCoreError> {
        self.refresh(settings);

        if let Some(mut bridge) = self.bridge.take() {
            bridge.stop();
        }

        let Some(mut child) = self.child.take() else {
            // We don't own a child process. If a proxy is nonetheless running
            // (adopted/orphaned, status Running from the health probe), terminate
            // it by binary name so Stop actually stops it instead of flipping
            // back to Running on the next refresh probe.
            if matches!(self.state.status, ProxyStatusKind::Running) {
                let bin_name = self
                    .paths
                    .managed_binary_path()
                    .file_name()
                    .and_then(|name| name.to_str())
                    .map(str::to_string)
                    .unwrap_or_else(|| "CLIProxyAPI".to_string());
                kill_process_by_name(&bin_name);
            }
            self.state = state_for_paths(
                settings,
                &self.paths,
                ProxyStatusKind::Stopped,
                None,
                None,
                self.crash_count,
                ProxyHealthState::unknown("代理核心未运行。"),
                "代理核心已停止。",
            );
            return Ok(());
        };

        let pid = child.id();
        self.state = state_for_paths(
            settings,
            &self.paths,
            ProxyStatusKind::Stopping,
            Some(pid),
            None,
            self.crash_count,
            self.state.health.clone(),
            "正在停止代理核心。",
        );

        if let Err(error) = child.kill() {
            if error.kind() != std::io::ErrorKind::InvalidInput {
                return Err(io_error("无法停止代理进程", error));
            }
        }

        let _ = child.wait();
        self.state = state_for_paths(
            settings,
            &self.paths,
            ProxyStatusKind::Stopped,
            None,
            None,
            self.crash_count,
            ProxyHealthState::unknown("代理核心已停止。"),
            "代理核心已停止。",
        );
        Ok(())
    }

    fn refresh(&mut self, settings: &AppSettings) {
        let Some(child) = self.child.as_mut() else {
            // We don't own a child process, but the proxy may still be running —
            // started by a previous app session, externally, or orphaned by an
            // app restart. Probe the management endpoint; if it answers, adopt it
            // as Running so the local management API (request logs / usage stats /
            // config) works regardless of who launched the proxy.
            let health = self.probe_health(settings);
            if health.ok == Some(true) {
                self.state = state_for_paths(
                    settings,
                    &self.paths,
                    ProxyStatusKind::Running,
                    None,
                    None,
                    self.crash_count,
                    health,
                    "检测到代理核心正在运行(非本会话启动)。",
                );
            } else {
                self.sync_settings(settings);
            }
            return;
        };

        match child.try_wait() {
            Ok(Some(exit_status)) => {
                let code = exit_status.code().unwrap_or(-1);
                self.child = None;
                self.crash_count = self.crash_count.saturating_add(1);
                self.state = state_for_paths(
                    settings,
                    &self.paths,
                    ProxyStatusKind::Crashed,
                    None,
                    Some(code),
                    self.crash_count,
                    ProxyHealthState::unhealthy(now_unix_seconds(), "代理进程已退出。"),
                    format!("代理进程已退出，退出码：{}。", code),
                );
            }
            Ok(None) => {
                let pid = self.child.as_ref().map(Child::id);
                self.state = state_for_paths(
                    settings,
                    &self.paths,
                    ProxyStatusKind::Running,
                    pid,
                    None,
                    self.crash_count,
                    self.state.health.clone(),
                    self.state.message.clone(),
                );
            }
            Err(error) => {
                self.state = state_for_paths(
                    settings,
                    &self.paths,
                    ProxyStatusKind::Error,
                    self.child.as_ref().map(Child::id),
                    None,
                    self.crash_count,
                    ProxyHealthState::unhealthy(now_unix_seconds(), "无法读取代理进程状态。"),
                    format!("无法读取代理进程状态：{}", error),
                );
            }
        }
    }

    fn check_health(&mut self, settings: &AppSettings) {
        self.refresh(settings);

        if self.child.is_none() {
            if !matches!(
                self.state.status,
                ProxyStatusKind::MissingBinary | ProxyStatusKind::Crashed
            ) {
                self.state.health = ProxyHealthState::unknown("代理核心未运行。");
                self.state.message = "代理核心未运行，无法执行健康检查。".to_string();
            }
            return;
        }

        let health = self.probe_health(settings);
        let message = if health.ok == Some(true) {
            "代理核心健康检查通过。"
        } else {
            "代理核心健康检查失败。"
        };
        self.state = state_for_paths(
            settings,
            &self.paths,
            ProxyStatusKind::Running,
            self.child.as_ref().map(Child::id),
            None,
            self.crash_count,
            health,
            message,
        );
    }

    fn probe_health(&self, settings: &AppSettings) -> ProxyHealthState {
        match probe_management_endpoint(settings, &self.management_key) {
            Ok(true) => ProxyHealthState::healthy(now_unix_seconds(), "管理接口可用。"),
            Ok(false) => {
                ProxyHealthState::unhealthy(now_unix_seconds(), "管理接口返回非成功状态。")
            }
            Err(message) => ProxyHealthState::unhealthy(now_unix_seconds(), message),
        }
    }

    fn write_config(&self, settings: &AppSettings) -> std::io::Result<()> {
        fs::create_dir_all(&self.paths.config_root)?;
        fs::create_dir_all(&self.paths.proxy_dir)?;
        fs::create_dir_all(&self.paths.auth_dir)?;

        let mut config = render_proxy_config(
            settings,
            &self.paths,
            &self.management_key,
            &self.local_api_key,
        );
        config.push_str(&custom_providers_yaml());
        fs::write(&self.paths.config_path, config)
    }
}

struct ProxyPaths {
    config_root: PathBuf,
    proxy_dir: PathBuf,
    config_path: PathBuf,
    auth_dir: PathBuf,
    resource_dir: PathBuf,
}

impl ProxyPaths {
    fn new() -> Self {
        let config_root = quotio_platform::app_config_dir();
        let proxy_dir = config_root.join("proxy");
        Self {
            config_path: config_root.join("config.yaml"),
            auth_dir: quotio_platform::proxy_auth_dir(),
            resource_dir: quotio_platform::proxy_resource_dir(),
            config_root,
            proxy_dir,
        }
    }

    fn resolve_resource_binary(&self) -> Option<PathBuf> {
        let entries = fs::read_dir(&self.resource_dir).ok()?;
        let mut files = entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.is_file())
            .collect::<Vec<_>>();

        files.sort();

        for candidate in proxy_binary_candidates() {
            if let Some(path) = files.iter().find(|path| file_name_eq(path, candidate)) {
                return Some(path.clone());
            }
        }

        files.into_iter().find(|path| looks_like_proxy_binary(path))
    }

    fn expected_resource_binary(&self) -> PathBuf {
        self.resource_dir.join(proxy_binary_candidates()[0])
    }

    fn managed_binary_path(&self) -> PathBuf {
        self.proxy_dir.join(proxy_binary_candidates()[0])
    }

    fn prepare_managed_binary(&self, resource_binary: &Path) -> std::io::Result<PathBuf> {
        fs::create_dir_all(&self.proxy_dir)?;
        let target = self.managed_binary_path();
        fs::copy(resource_binary, &target)?;
        make_executable(&target)?;
        Ok(target)
    }
}

fn state_for_paths(
    settings: &AppSettings,
    paths: &ProxyPaths,
    status: ProxyStatusKind,
    pid: Option<u32>,
    exit_code: Option<i32>,
    crash_count: u32,
    health: ProxyHealthState,
    message: impl Into<String>,
) -> ProxyState {
    ProxyState {
        status,
        endpoint: settings.endpoint(),
        management_endpoint: settings.management_endpoint(),
        pid,
        binary_path: Some(paths.managed_binary_path().display().to_string()),
        config_path: Some(paths.config_path.display().to_string()),
        auth_dir: Some(paths.auth_dir.display().to_string()),
        resource_dir: Some(paths.resource_dir.display().to_string()),
        exit_code,
        crash_count,
        health,
        message: message.into(),
    }
}

fn missing_binary_state(
    settings: &AppSettings,
    paths: &ProxyPaths,
    crash_count: u32,
) -> ProxyState {
    let expected_path = paths.expected_resource_binary().display().to_string();
    ProxyState {
        status: ProxyStatusKind::MissingBinary,
        endpoint: settings.endpoint(),
        management_endpoint: settings.management_endpoint(),
        pid: None,
        binary_path: Some(expected_path),
        config_path: Some(paths.config_path.display().to_string()),
        auth_dir: Some(paths.auth_dir.display().to_string()),
        resource_dir: Some(paths.resource_dir.display().to_string()),
        exit_code: None,
        crash_count,
        health: ProxyHealthState::unknown("缺少可执行文件，无法检查健康状态。"),
        message: "未找到当前平台可用的 CLIProxyAPI 二进制。".to_string(),
    }
}

fn proxy_resource_status(paths: &ProxyPaths) -> ProxyResourceStatus {
    let current_platform = quotio_platform::current_proxy_platform().to_string();
    let resource_root = paths
        .resource_dir
        .parent()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| paths.resource_dir.display().to_string());
    let expected_binary_names = proxy_binary_candidates()
        .iter()
        .map(|name| (*name).to_string())
        .collect::<Vec<_>>();
    let detected_binary_path = paths
        .resolve_resource_binary()
        .map(|path| path.display().to_string());
    let platforms = ["windows", "darwin", "linux"]
        .into_iter()
        .map(|platform| proxy_platform_resource_status(paths, platform))
        .collect::<Vec<_>>();
    let has_current_platform_binary = detected_binary_path.is_some();
    let message = if has_current_platform_binary {
        "当前平台代理二进制已就绪。"
    } else {
        "当前平台缺少代理二进制，真实运行态仍不可启动。"
    }
    .to_string();

    ProxyResourceStatus {
        current_platform,
        resource_root,
        current_resource_dir: paths.resource_dir.display().to_string(),
        expected_binary_names,
        detected_binary_path,
        has_current_platform_binary,
        platforms,
        message,
    }
}

fn proxy_platform_resource_status(
    paths: &ProxyPaths,
    platform: &str,
) -> ProxyPlatformResourceStatus {
    let directory = paths
        .resource_dir
        .parent()
        .map(|root| root.join(platform))
        .unwrap_or_else(|| paths.resource_dir.clone());
    let files = resource_file_names(&directory);
    let detected_binary_path = files
        .iter()
        .find(|file| {
            proxy_binary_candidates_for_platform(platform)
                .iter()
                .any(|candidate| file.eq_ignore_ascii_case(candidate))
        })
        .or_else(|| {
            files
                .iter()
                .find(|file| looks_like_proxy_binary_name(file, platform))
        })
        .map(|file| directory.join(file).display().to_string());

    ProxyPlatformResourceStatus {
        platform: platform.to_string(),
        directory: directory.display().to_string(),
        files,
        has_binary: detected_binary_path.is_some(),
        detected_binary_path,
    }
}

fn resource_file_names(directory: &Path) -> Vec<String> {
    let Ok(entries) = fs::read_dir(directory) else {
        return Vec::new();
    };

    let mut files = entries
        .filter_map(Result::ok)
        .filter(|entry| entry.path().is_file())
        .filter_map(|entry| entry.file_name().to_str().map(ToOwned::to_owned))
        .collect::<Vec<_>>();
    files.sort();
    files
}

fn proxy_binary_candidates() -> &'static [&'static str] {
    proxy_binary_candidates_for_platform(quotio_platform::current_proxy_platform())
}

fn proxy_binary_candidates_for_platform(platform: &str) -> &'static [&'static str] {
    if platform == "windows" {
        &[
            "CLIProxyAPI.exe",
            "cli-proxy-api-plus.exe",
            "cli-proxy-api.exe",
            "claude-code-proxy.exe",
            "proxy.exe",
        ]
    } else {
        &[
            "CLIProxyAPI",
            "cli-proxy-api-plus",
            "cli-proxy-api",
            "claude-code-proxy",
            "proxy",
        ]
    }
}

fn file_name_eq(path: &Path, candidate: &str) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.eq_ignore_ascii_case(candidate))
        .unwrap_or(false)
}

fn looks_like_proxy_binary(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(|name| looks_like_proxy_binary_name(name, quotio_platform::current_proxy_platform()))
        .unwrap_or(false)
}

fn looks_like_proxy_binary_name(file_name: &str, platform: &str) -> bool {
    let lower = file_name.to_ascii_lowercase();

    if lower.ends_with(".md")
        || lower.ends_with(".txt")
        || lower.ends_with(".json")
        || lower.ends_with(".yaml")
        || lower.ends_with(".yml")
        || lower.ends_with(".sh")
    {
        return false;
    }

    if platform == "windows" {
        return lower.ends_with(".exe");
    }

    true
}

fn make_executable(path: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(path)?.permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions)?;
    }

    #[cfg(not(unix))]
    {
        let _ = path;
    }

    Ok(())
}

fn render_proxy_config(
    settings: &AppSettings,
    paths: &ProxyPaths,
    management_key: &str,
    _local_api_key: &str,
) -> String {
    let keys = get_api_keys();
    let api_keys_block = if keys.is_empty() {
        "  - ''".to_string()
    } else {
        keys.iter()
            .map(|key| format!("  - {}", yaml_quote(key)))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let mut config = format!(
        "host: {}\nport: {}\nauth-dir: {}\nproxy-url: {}\n\napi-keys:\n{}\n\nremote-management:\n  allow-remote: {}\n  secret-key: {}\n\ndebug: {}\nlogging-to-file: {}\nlogs-max-total-size-mb: {}\nusage-statistics-enabled: true\ndisable-cooling: {}\ndisable-image-generation: {}\nforce-model-prefix: {}\npassthrough-headers: {}\n\nrouting:\n  strategy: {}\n  session-affinity: {}\n  session-affinity-ttl: {}\n\nquota-exceeded:\n  switch-project: true\n  switch-preview-model: true\n\nrequest-retry: {}\nmax-retry-credentials: {}\nmax-retry-interval: {}\n",
        yaml_quote(&bind_host(settings)),
        settings.proxy_port,
        yaml_quote(&paths.auth_dir.display().to_string()),
        yaml_quote(settings.proxy_url.trim()),
        api_keys_block,
        settings.allow_remote,
        yaml_quote(management_key),
        settings.debug,
        settings.logging_to_file,
        settings.logs_max_total_size_mb,
        settings.disable_cooling,
        settings.disable_image_generation,
        settings.force_model_prefix,
        settings.passthrough_headers,
        yaml_quote(routing_strategy_value(&settings.routing_strategy)),
        settings.session_affinity,
        yaml_quote(settings.session_affinity_ttl.trim()),
        settings.request_retry,
        settings.max_retry_credentials,
        settings.max_retry_interval_seconds,
    );
    config.push_str(&render_payload_overrides(settings));
    config
}

/// Render a global `payload.override` rule from the visual model/reasoning
/// settings, so CLIProxyAPI rewrites incoming requests (force a model and/or a
/// reasoning effort) as they arrive. Returns "" when neither is configured.
fn render_payload_overrides(settings: &AppSettings) -> String {
    let reasoning = settings.reasoning_effort.trim();
    let force_model = settings.force_model.trim();
    let mut params = Vec::new();
    if !force_model.is_empty() {
        params.push(format!("        \"model\": {}", yaml_quote(force_model)));
    }
    if !reasoning.is_empty() {
        params.push(format!("        \"reasoning.effort\": {}", yaml_quote(reasoning)));
    }
    if params.is_empty() {
        return String::new();
    }
    format!(
        "\npayload:\n  override:\n    - models:\n        - name: \"*\"\n      params:\n{}\n",
        params.join("\n")
    )
}

fn routing_strategy_value(strategy: &RoutingStrategy) -> &'static str {
    match strategy {
        RoutingStrategy::RoundRobin => "round-robin",
        RoutingStrategy::FillFirst => "fill-first",
    }
}

fn yaml_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn bind_host(settings: &AppSettings) -> String {
    if settings.allow_remote {
        return "0.0.0.0".to_string();
    }

    let host = settings.proxy_host.trim();
    if host.is_empty() {
        "127.0.0.1".to_string()
    } else {
        host.to_string()
    }
}

fn health_connect_host(settings: &AppSettings) -> String {
    let host = settings.proxy_host.trim();
    if host.is_empty() || host == "0.0.0.0" || host == "::" {
        "127.0.0.1".to_string()
    } else {
        host.to_string()
    }
}

/// Best-effort terminate a proxy process by image/binary name. Used to stop a
/// proxy this app session does not own (adopted from a previous session, an
/// external launch, or orphaned by a restart) so the Stop button actually works.
fn kill_process_by_name(name: &str) {
    let name = name.trim();
    if name.is_empty() {
        return;
    }
    #[cfg(windows)]
    {
        let _ = std::process::Command::new("taskkill")
            .args(["/F", "/IM", name])
            .output();
    }
    #[cfg(not(windows))]
    {
        let _ = std::process::Command::new("pkill").args(["-f", name]).output();
    }
}

fn probe_management_endpoint(settings: &AppSettings, management_key: &str) -> Result<bool, String> {
    let host = health_connect_host(settings);
    let address = format!("{}:{}", host, settings.proxy_port);
    let mut addrs = address
        .to_socket_addrs()
        .map_err(|error| format!("无法解析代理地址：{}", error))?;
    let Some(addr) = addrs.next() else {
        return Err("无法解析代理地址。".to_string());
    };

    let timeout = Duration::from_secs(3);
    let mut stream = TcpStream::connect_timeout(&addr, timeout)
        .map_err(|error| format!("无法连接代理管理接口：{}", error))?;
    stream
        .set_read_timeout(Some(timeout))
        .map_err(|error| format!("无法设置读取超时：{}", error))?;
    stream
        .set_write_timeout(Some(timeout))
        .map_err(|error| format!("无法设置写入超时：{}", error))?;

    let request = format!(
        "GET /v0/management/debug HTTP/1.1\r\nHost: {}:{}\r\nAuthorization: Bearer {}\r\nAccept: application/json\r\nConnection: close\r\n\r\n",
        host, settings.proxy_port, management_key
    );
    stream
        .write_all(request.as_bytes())
        .map_err(|error| format!("无法发送健康检查请求：{}", error))?;

    let mut buffer = [0_u8; 512];
    let size = stream
        .read(&mut buffer)
        .map_err(|error| format!("无法读取健康检查响应：{}", error))?;
    let response = String::from_utf8_lossy(&buffer[..size]);

    Ok(response.starts_with("HTTP/1.1 2") || response.starts_with("HTTP/1.0 2"))
}

fn now_unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use quotio_types::{ConnectionMode, MigrationPhase};
    use std::{
        net::{TcpListener, TcpStream},
        sync::{Arc, Mutex},
        thread,
        time::{Duration, Instant},
    };

    #[test]
    fn proxy_resource_status_ignores_readme_only_platform_dir() {
        let temp_root =
            std::env::temp_dir().join(format!("quotio-proxy-resource-test-{}", Uuid::new_v4()));
        let platform_dir = temp_root.join(quotio_platform::current_proxy_platform());
        fs::create_dir_all(&platform_dir).expect("platform resource dir should be created");
        fs::write(platform_dir.join("README.md"), "placeholder")
            .expect("placeholder should be written");

        let paths = ProxyPaths {
            config_root: temp_root.join("config"),
            proxy_dir: temp_root.join("managed"),
            config_path: temp_root.join("config").join("config.yaml"),
            auth_dir: temp_root.join("auth"),
            resource_dir: platform_dir,
        };

        let status = proxy_resource_status(&paths);

        assert!(!status.has_current_platform_binary);
        assert!(status.detected_binary_path.is_none());
        assert!(status.platforms.iter().any(|platform| {
            platform.platform == quotio_platform::current_proxy_platform()
                && !platform.has_binary
                && platform.files == vec!["README.md".to_string()]
        }));

        let _ = fs::remove_dir_all(temp_root);
    }

    #[tokio::test]
    async fn app_core_rejects_local_management_refresh_when_proxy_is_not_running() {
        let mut core = AppCore::default();
        core.settings.connection_mode = ConnectionMode::Local;
        core.settings.remote_endpoint_url = None;
        core.settings.remote_management_key = None;

        let error = core
            .management_client()
            .expect_err("local management should require a running proxy");

        assert!(error.to_string().contains("代理核心未运行"));
    }

    #[tokio::test]
    async fn app_core_refreshes_management_snapshot_from_configured_endpoint() {
        let server = FakeManagementServer::new(vec![
            FakeResponse::json(
                200,
                r#"{"files":[{"id":"claude-1","name":"claude-user.json","provider":"claude","status":"ready","disabled":false,"unavailable":false}]}"#,
            ),
            FakeResponse::json(
                200,
                r#"{"usage":{"total_requests":5,"success_count":4,"failure_count":1,"input_tokens":30,"output_tokens":20},"failed_requests":1}"#,
            ),
            FakeResponse::json(200, r#"{"api-keys":["sk-local-secret"]}"#),
            FakeResponse::json(
                200,
                r#"{"debug":true,"routing-strategy":"fill-first","request-retry":4,"max-retry-interval":45,"request-log":true}"#,
            ),
            FakeResponse::json(
                200,
                r#"{"lines":["started"],"line-count":1,"latest-timestamp":10}"#,
            ),
        ]);
        let mut core = AppCore::default();
        core.settings.connection_mode = ConnectionMode::Remote;
        core.settings.remote_endpoint_url = Some(server.base_url());
        core.settings.remote_management_key = Some("remote-secret".to_string());
        core.proxy.sync_settings(&core.settings);

        let client = core
            .management_client()
            .expect("configured management endpoint should create a client");
        let snapshot = client
            .refresh_snapshot()
            .await
            .expect("management snapshot should refresh");
        let state = core.apply_management_snapshot(snapshot);

        assert_eq!(state.migration_phase, MigrationPhase::PlatformAdapters);
        assert_eq!(state.management.auth_files.len(), 1);
        assert_eq!(state.auth_files.len(), 1);
        assert_eq!(state.management.api_keys, vec!["sk-local-secret"]);
        assert_eq!(state.api_keys[0].masked_value, "sk-l••••cret");
        assert_eq!(
            state
                .request_stats
                .as_ref()
                .map(|stats| stats.total_requests),
            Some(5)
        );
        assert_eq!(
            state
                .management
                .config
                .as_ref()
                .and_then(|config| config.routing_strategy.as_deref()),
            Some("fill-first")
        );

        let requests = server.requests();
        assert_eq!(
            requests
                .iter()
                .map(|request| request.path.as_str())
                .collect::<Vec<_>>(),
            vec![
                "/v0/management/auth-files",
                "/v0/management/usage",
                "/v0/management/api-keys",
                "/v0/management/config",
                "/v0/management/logs",
            ]
        );
        assert!(requests.iter().all(|request| request
            .headers
            .iter()
            .any(|header| header == "Authorization: Bearer remote-secret")));
    }

    #[derive(Clone, Debug)]
    struct RecordedRequest {
        path: String,
        headers: Vec<String>,
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
        let (head, _) = raw.split_once("\r\n\r\n").unwrap_or((&raw, ""));
        let mut lines = head.lines();
        let request_line = lines.next().unwrap_or_default();
        let path = request_line
            .split_whitespace()
            .nth(1)
            .unwrap_or_default()
            .to_string();
        let headers = lines.map(ToOwned::to_owned).collect();

        RecordedRequest { path, headers }
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
