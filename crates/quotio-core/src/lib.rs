pub mod agent_config;
pub mod agents;
pub mod bridge;
pub mod codex_launch;
pub mod codex_session_visibility;
pub mod kiro_sidecar;
pub mod management;
pub mod native_oauth;
pub mod proxy_download;
pub mod quota;
pub mod scheduler;
pub mod tunnel;
pub mod usage_store;

use std::{
    fs,
    io::{Read as _, Write as _},
    net::{TcpStream, ToSocketAddrs},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::Arc,
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use management::{ManagementApiClient, ManagementApiError};
use quotio_types::{
    default_available_models, default_providers, mask_secret, AccountAuthHealth, AccountQuota,
    AccountSummaryRow, AgentBackupFile, AgentConfigMode, AgentConfigStorageOption,
    AgentConfigurationRequest, AgentConfigurationResult, AgentSetupMode, ApiKeyBinding, ApiKeyEntry, AppSettings,
    AppState, AuthFile, AvailableModel, ConnectionMode, CredentialStatus, FallbackConfigAction,
    FallbackConfiguration, FallbackEntry, FallbackEntryMoveDirection, FallbackRouteState,
    FallbackRuntimeState, ManagementSnapshot, MigrationPhase, ModelPrice, ModelSlot, PlatformInfo,
    ProxyHealthState, ProxyPlatformResourceStatus, ProxyResourceStatus, ProxyState,
    ProxyStatusKind, RequestStats, RoutingStrategy, SavedAgentConfiguration, UsageAggregate,
    UsageChartBucket, UsageFilterOptions, UsageModelBreakdownRow, UsageQuery, UsageTimeSeriesPoint,
    VirtualModel,
};
use usage_store::UsageStore;
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
    /// Persistent request-level usage history (SQLite). Shared with the
    /// background collector that drains the proxy's usage queue.
    usage_store: Arc<UsageStore>,
    credential_error: Option<String>,
    /// 当前 Codex 一键启动会话（启动时建立，停止/关软件时还原成启动前的样子）。
    codex_session: Option<codex_launch::CodexSession>,
    /// 会话代数：每次启动 +1。监控的进程探测在锁外执行，
    /// 写回结果时校验代数，期间停止/重启过就丢弃这次探测。
    codex_session_generation: u64,
    /// 当前在跑的启动方案 id（与 codex_session 同生命周期；停止/自动退出时清空）。
    codex_active_profile_id: Option<String>,
    /// 每个服务商的调度状态。
    schedulers: std::collections::HashMap<String, ProviderSchedulerState>,
}

struct ProviderSchedulerState {
    current: Option<(String, Instant)>,
    target_label: Option<String>,
    current_reset_at: Option<i64>,
    standby_count: u32,
    failure_recheck_at: Option<Instant>,
    /// 账号 key → 因额度耗尽(余量 ≤ 3%)被踢出可选集的账号。带迟滞:余量回到
    /// > 5%(刷新后)才移除。避免在阈值边界反复横跳。
    exhausted: std::collections::HashSet<String>,
}

/// Codex 监控的进程探测目标。由 [`AppCore::codex_monitor_probe`] 在锁内产出，
/// [`CodexMonitorProbe::run`] 在锁外执行（会 spawn tasklist，几十毫秒），
/// 结果再经 [`AppCore::codex_monitor_apply`] 写回。
#[derive(Debug, Clone, Copy)]
pub enum CodexMonitorProbe {
    /// 按进程名查 Codex 桌面应用（App 模式）。
    AppByName,
    /// 按 pid 查启动的终端进程（CLI 模式）。
    CliByPid(u32),
}

impl CodexMonitorProbe {
    /// 执行实际的进程探测。调用方应在不持有 core 锁时调用。
    pub fn run(&self) -> bool {
        match self {
            CodexMonitorProbe::AppByName => codex_launch::codex_app_process_running(),
            CodexMonitorProbe::CliByPid(pid) => codex_launch::process_alive(*pid),
        }
    }
}

impl Default for AppCore {
    fn default() -> Self {
        let mut settings = read_settings().unwrap_or_default();
        migrate_codex_profiles(&mut settings);
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
            usage_store: open_usage_store(),
            credential_error,
            codex_session: None,
            codex_session_generation: 0,
            codex_active_profile_id: None,
            schedulers: std::collections::HashMap::new(),
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

    /// Release the proxy on app exit: stop a Quotio-started child + fallback
    /// bridge, and terminate an adopted/external proxy by its port, so closing
    /// the app doesn't leave the proxy API running in the background.
    pub fn shutdown(&mut self) {
        // 关闭软件时默认还原 Codex 注入：杀掉启动的进程 + 从固定备份文件还原。
        if let Some(session) = self.codex_session.take() {
            if let Some(pid) = session.pid {
                codex_launch::kill_process(pid);
            }
            codex_launch::close_codex_app();
            let _ = codex_launch::restore_codex_state_from_launch_backup();
        } else if codex_launch::launch_backup_exists() {
            codex_launch::close_codex_app();
            let _ = codex_launch::restore_codex_state_from_launch_backup();
        }
        // 退出时恢复被调度临时禁用的账号，别让池子带着 standby 状态过夜。
        let _ = scheduler::release_all_in(&quotio_platform::proxy_auth_dir());
        self.proxy.shutdown(&self.settings);
    }

    pub fn restart_proxy(&mut self) -> Result<AppState, ProxyCoreError> {
        self.proxy.stop(&self.settings)?;
        thread::sleep(Duration::from_millis(250));
        self.proxy.start(&self.settings)?;
        Ok(self.app_state())
    }

    pub fn rewrite_proxy_config(&self) {
        let _ = self.proxy.write_config(&self.settings);
    }

    pub fn proxy_managed_binary_path(&self) -> PathBuf {
        self.proxy.managed_binary_path()
    }

    /// Path where the managed cloudflared binary lives (next to the proxy core).
    pub fn cloudflared_binary_path(&self) -> PathBuf {
        let binary_name = if cfg!(target_os = "windows") {
            "cloudflared.exe"
        } else {
            "cloudflared"
        };
        self.proxy_managed_binary_path()
            .parent()
            .map(|dir| dir.join(binary_name))
            .unwrap_or_else(|| PathBuf::from(binary_name))
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

        let management_endpoint = match self.settings.connection_mode {
            ConnectionMode::Local => self.proxy.state.management_endpoint.clone(),
            ConnectionMode::Remote => {
                if self
                    .settings
                    .remote_endpoint_url
                    .as_deref()
                    .map(str::trim)
                    .filter(|endpoint| !endpoint.is_empty())
                    .is_none()
                {
                    return Err(ManagementCoreError::Unavailable(
                        "远程管理接口地址未配置。".to_string(),
                    ));
                }
                self.settings.management_endpoint()
            }
        };

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
            management_endpoint,
            management_key,
        ))
    }

    pub async fn refresh_management_snapshot(&mut self) -> Result<AppState, ManagementCoreError> {
        let client = self.management_client()?;
        // Enable per-request telemetry so the usage queue fills (idempotent).
        // Draining the queue is owned by the background collector — the single
        // consumer that persists events to the usage store — so this snapshot
        // refresh no longer competes for the destructive `/usage-queue` read.
        let _ = client.set_usage_statistics_enabled(true).await;
        let snapshot = client.refresh_snapshot().await?;
        Ok(self.apply_management_snapshot(snapshot))
    }

    /// Shared handle to the persistent usage store, for the background collector
    /// that drains the proxy's (destructive) usage queue and persists events.
    pub fn usage_store(&self) -> Arc<UsageStore> {
        self.usage_store.clone()
    }

    /// Aggregated KPI totals for the dashboard, over a filtered time range.
    pub fn query_usage_stats(&self, query: &UsageQuery) -> UsageAggregate {
        self.usage_store.query_stats(query)
    }

    /// Per-account rollup for the dashboard summary table.
    pub fn query_account_summary(&self, query: &UsageQuery) -> Vec<AccountSummaryRow> {
        self.usage_store.account_summary(query)
    }

    /// Time-bucketed rollup for the dashboard usage charts.
    pub fn query_usage_timeseries(
        &self,
        query: &UsageQuery,
        bucket: UsageChartBucket,
    ) -> Vec<UsageTimeSeriesPoint> {
        self.usage_store.usage_timeseries(query, bucket)
    }

    /// Per-model rollup for the dashboard model ranking chart.
    pub fn query_usage_model_breakdown(
        &self,
        query: &UsageQuery,
        limit: usize,
    ) -> Vec<UsageModelBreakdownRow> {
        self.usage_store.model_breakdown(query, limit)
    }

    /// Distinct filter values for the dashboard dropdowns.
    pub fn usage_filter_options(&self) -> UsageFilterOptions {
        self.usage_store.filter_options()
    }

    /// Per-account auth health (genuine 401/403 vs rate-limit/server errors),
    /// so the accounts panel only suggests re-auth on real auth failures.
    pub fn account_auth_health(&self) -> Vec<AccountAuthHealth> {
        self.usage_store.account_auth_health(20)
    }

    /// Configured model prices for cost estimation.
    pub fn model_prices(&self) -> Vec<ModelPrice> {
        self.usage_store.model_prices()
    }

    /// Replace the configured model prices.
    pub fn set_model_prices(&self, prices: &[ModelPrice]) {
        self.usage_store.set_model_prices(prices);
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
        self.store_quotas(quotas);
        self.app_state()
    }

    /// 只存配额不生成快照（调用方随后自己跑 [`Self::scheduler_reconcile`] + `app_state`）。
    pub fn store_quotas(&mut self, quotas: Vec<AccountQuota>) {
        // Preserve last-known-good quota. A probe routed through a flaky upstream
        // proxy often comes back blank (no models, not forbidden, not auth-failed)
        // for some accounts; blindly storing that flaps each card to "额度获取失败"
        // and back every poll. So when SOME accounts succeeded, keep the previous
        // numbers for the ones that came back transiently blank. If EVERY account
        // is blank, store as-is — that's a real "proxy unreachable" the UI surfaces
        // (rather than masking an outage behind stale numbers).
        fn is_blank(account: &AccountQuota) -> bool {
            account.models.is_empty()
                && !account.is_forbidden
                && account.status_message.as_deref() != Some("auth_failed")
        }
        if quotas.iter().all(is_blank) {
            self.quotas = quotas;
            return;
        }
        let previous = std::mem::take(&mut self.quotas);
        self.quotas = quotas
            .into_iter()
            .map(|account| {
                if is_blank(&account) {
                    if let Some(old) = previous.iter().find(|old| {
                        old.provider_id == account.provider_id
                            && old.account_key == account.account_key
                    }) {
                        if !old.models.is_empty() {
                            return old.clone();
                        }
                    }
                }
                account
            })
            .collect();
    }

    /// 智能调度评估 + 守门执行。每次配额刷新后调用；规则关闭时负责把
    /// standby 账号放回池子（fail-open 同理）。返回是否改动了池子状态。
    pub fn scheduler_reconcile(&mut self) -> bool {
        let dir = quotio_platform::proxy_auth_dir();
        if self.settings.scheduler_rule != "reset_soonest" {
            let changed = scheduler::release_all_in(&dir);
            self.schedulers.clear();
            return changed;
        }

        let providers = scheduler::discover_schedulable_providers(&dir);
        let now_unix = now_unix_seconds() as i64;
        let min_hold = Duration::from_secs(self.settings.scheduler_min_hold_minutes as u64 * 60);
        let margin = self.settings.scheduler_switch_margin_minutes as i64 * 60;
        let mut any_changed = false;

        // 清理已不存在的服务商。
        self.schedulers.retain(|pid, _| providers.contains(pid));

        for provider_id in &providers {
            let pool = scheduler::read_pool_for_provider(&dir, provider_id);
            let mut candidates =
                scheduler::build_candidates(&pool, &self.quotas, now_unix, provider_id);

            let state = self
                .schedulers
                .entry(provider_id.clone())
                .or_insert_with(|| ProviderSchedulerState {
                    current: None,
                    target_label: None,
                    current_reset_at: None,
                    standby_count: 0,
                    failure_recheck_at: None,
                    exhausted: Default::default(),
                });

            // 额度耗尽迟滞:把余量 ≤3% 的号踢出可选(状态记在 state.exhausted,余量
            // 回 >5% 才放回),让 pick_target 改选满血待命号,而不是死磕快耗尽的号。
            scheduler::apply_exhaustion_hysteresis(&mut candidates, &mut state.exhausted);

            let current = state
                .current
                .as_ref()
                .map(|(file, since)| (file.as_str(), since.elapsed()));

            let Some(target) = scheduler::pick_target(&candidates, current, min_hold, margin)
            else {
                let changed = scheduler::release_provider_in(&dir, provider_id);
                state.current = None;
                state.target_label = None;
                state.current_reset_at = None;
                state.standby_count = 0;
                any_changed |= changed;
                continue;
            };

            let target_changed = state
                .current
                .as_ref()
                .map(|(file, _)| file != &target)
                .unwrap_or(true);
            if target_changed {
                state.current = Some((target.clone(), Instant::now()));
            }
            let (pool_changed, standby_count) = scheduler::apply_target_in(&dir, &pool, &target);

            let picked = candidates.iter().find(|c| c.file_name == target);
            state.target_label = picked.map(|c| c.label.clone());
            state.current_reset_at = picked.and_then(|c| c.session_reset_at);
            state.standby_count = standby_count;
            any_changed |= target_changed || pool_changed;
        }
        any_changed
    }

    /// 用量事件里出现了**当前目标账号**的失败请求：
    /// 返回 true 表示该立刻重拉配额重选。带 60 秒冷却。
    pub fn scheduler_should_recheck_for_failures(
        &mut self,
        events: &[quotio_types::UsageEvent],
    ) -> bool {
        if self.settings.scheduler_rule != "reset_soonest" {
            return false;
        }
        let mut should_recheck = false;
        for state in self.schedulers.values_mut() {
            let Some(label) = state.target_label.as_deref() else {
                continue;
            };
            let target_failed = events
                .iter()
                .any(|event| event.failed && event.source.as_deref() == Some(label));
            if !target_failed {
                continue;
            }
            if let Some(last) = state.failure_recheck_at {
                if last.elapsed() < Duration::from_secs(60) {
                    continue;
                }
            }
            state.failure_recheck_at = Some(Instant::now());
            should_recheck = true;
        }
        should_recheck
    }

    /// 任一服务商的选中账号额度是否已刷新（后台线程用：到点提前触发重评估）。
    pub fn scheduler_reset_due(&self) -> bool {
        if self.settings.scheduler_rule != "reset_soonest" {
            return false;
        }
        let now = now_unix_seconds() as i64;
        self.schedulers.values().any(|state| {
            state.current.is_some()
                && state
                    .current_reset_at
                    .map(|reset| reset <= now)
                    .unwrap_or(false)
        })
    }

    /// 调度状态快照（给前端展示）。
    fn scheduler_status(&self) -> quotio_types::SchedulerStatus {
        let entries: Vec<quotio_types::ProviderSchedulerEntry> = self
            .schedulers
            .iter()
            .map(|(pid, state)| quotio_types::ProviderSchedulerEntry {
                provider_id: pid.clone(),
                target_label: state.target_label.clone(),
                target_reset_at_unix: state.current_reset_at,
                standby_count: state.standby_count,
            })
            .collect();
        let first = entries.first();
        quotio_types::SchedulerStatus {
            rule: self.settings.scheduler_rule.clone(),
            target_label: first.and_then(|e| e.target_label.clone()),
            target_reset_at_unix: first.and_then(|e| e.target_reset_at_unix),
            standby_count: entries.iter().map(|e| e.standby_count).sum(),
            providers: entries,
        }
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
        let parsed: serde_json::Value = serde_json::from_str(content)
            .map_err(|_| "文件内容不是有效的 JSON".to_string())?;
        let dir = quotio_platform::proxy_auth_dir();
        std::fs::create_dir_all(&dir).map_err(|error| format!("创建 auth 目录失败：{}", error))?;

        // sub2api-data export: tokens are nested under each account's
        // `credentials`, so expand accounts[] and flatten/convert each to a
        // CLIProxyAPI auth object before writing (openai → codex).
        if let Some(items) = convert_sub2api_accounts(&parsed) {
            for item in &items {
                self.write_single_auth_import(&dir, item, base);
            }
        // accounts-export.json: { accounts: [{ token, refreshToken, email, accountId, … }] }
        // —— 扁平驼峰、无 id_token,转换成 codex auth 后逐个落地。
        } else if let Some(items) = convert_accounts_export(&parsed) {
            for item in &items {
                self.write_single_auth_import(&dir, item, base);
            }
        // If the file is a JSON array (e.g. cpa-manager batch export), unpack
        // each element into its own auth file with a proper provider-email name.
        } else if let Some(arr) = parsed.as_array() {
            if arr.is_empty() {
                return Err("导入文件为空数组".to_string());
            }
            for item in arr {
                if !item.is_object() {
                    continue;
                }
                self.write_single_auth_import(&dir, item, base);
            }
        } else if parsed.is_object() {
            self.write_single_auth_import(&dir, &parsed, base);
        } else {
            return Err("JSON 内容必须是对象或数组".to_string());
        }

        dedup_codex_auth_keep_newest(&dir, &self.settings.codex_bound_account);
        Ok(self.app_state())
    }

    fn write_single_auth_import(
        &self,
        dir: &std::path::Path,
        item: &serde_json::Value,
        original_filename: &str,
    ) {
        let obj = match item.as_object() {
            Some(o) => o,
            None => return,
        };

        // Determine provider type from the object or filename.
        let provider = obj
            .get("type")
            .or_else(|| obj.get("provider"))
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .or_else(|| {
                // Guess from the "id" field: "kiro_xxx" → "kiro", "codex_xxx" → "codex"
                obj.get("id")
                    .and_then(|v| v.as_str())
                    .and_then(|id| id.split('_').next())
                    .map(str::to_string)
            })
            .unwrap_or_else(|| {
                // Last resort: filename prefix before first _ or -
                let stem = original_filename.trim_end_matches(".json").trim_end_matches(".JSON");
                stem.split(|c: char| c == '_' || c == '-')
                    .next()
                    .unwrap_or(stem)
                    .to_string()
            });

        // Build a clean auth object with `type` always set.
        let mut output = item.clone();
        if let Some(out_obj) = output.as_object_mut() {
            out_obj
                .entry("type".to_string())
                .or_insert_with(|| serde_json::Value::String(provider.clone()));
        }

        // 文件名 = <provider>-<唯一标识>.json。标识见 auth_file_ident:多份「没 email」的账号
        // 不再都落到 <provider>-unknown.json 互相覆盖(= 多选导入只进来一个的根因)。
        let target_name = format!("{}-{}.json", provider, auth_file_ident(item));
        let target = dir.join(&target_name);

        if let Ok(json) = serde_json::to_string_pretty(&output) {
            let _ = std::fs::write(&target, json);
        }
    }

    /// De-duplicate codex auth files by account identity (keep bound / newest),
    /// so re-importing or re-logging-in the same account never leaves two files
    /// (= two cards, two pool entries). Best-effort; safe to call repeatedly.
    pub fn dedup_codex_auth(&self) {
        dedup_codex_auth_keep_newest(
            &quotio_platform::proxy_auth_dir(),
            &self.settings.codex_bound_account,
        );
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

    /// 一键启动 Codex：备份原始 auth.json/config.toml → 写代理配置 → 注入绑定账号 → 启动 App/CLI。
    /// 参数全部来自已保存的设置（codex_* 字段）。
    pub fn codex_start(&mut self, profile_id: &str) -> Result<String, ManagementCoreError> {
        let profile_id = profile_id.trim();
        // 找到要启动的方案（按 id）。
        let profile = self
            .settings
            .codex_profiles
            .iter()
            .find(|profile| profile.id == profile_id)
            .cloned()
            .ok_or_else(|| {
                ManagementCoreError::Unavailable("找不到该启动方案，请刷新后重试".to_string())
            })?;

        // 已经在跑：同一套幂等返回；不同套先停掉当前的（还原配置、释放占用）再起新的。
        if self.codex_session.is_some() {
            if self.codex_active_profile_id.as_deref() == Some(profile_id) {
                return Ok("该方案已在运行".to_string());
            }
            self.codex_stop()?;
        }

        let account_key = profile.bound_account.trim().to_string();
        if account_key.is_empty() {
            return Err(ManagementCoreError::Unavailable(
                "该方案还没有绑定 Codex 账号，请先编辑方案选择账号".to_string(),
            ));
        }

        // If Quotio was closed or a previous launch failed after creating the
        // launch backup, no in-memory session exists but the backup file still
        // blocks a new launch. Restore first so startup is self-healing.
        if codex_launch::launch_backup_exists() {
            codex_launch::close_codex_app();
            thread::sleep(Duration::from_millis(400));
            codex_launch::restore_codex_state_from_launch_backup().map_err(|error| {
                ManagementCoreError::Unavailable(format!(
                    "检测到上次 Codex 启动未完成，自动恢复失败：{}",
                    error
                ))
            })?;
        }

        codex_launch::mark_bound_account_login_only(&account_key)
            .map_err(ManagementCoreError::Unavailable)?;
        // 绑定占用可能正好拿走了调度器当前选中的账号：立刻重选，
        // 避免「目标被绑定 + 其余都在待命」导致代理池空窗。
        let _ = self.scheduler_reconcile();
        if !matches!(
            self.app_state().proxy.status,
            ProxyStatusKind::Running | ProxyStatusKind::Starting
        ) {
            self.start_proxy().map_err(|error| {
                ManagementCoreError::Unavailable(format!("启动代理失败：{error}"))
            })?;
        }
        let local_endpoint = self.app_state().proxy.endpoint.clone();
        // 方案指定了代理地址就用它（让不同方案走不同账号池）；留空回退本机端点。
        let proxy_url = if profile.proxy_url.trim().is_empty() {
            local_endpoint
        } else {
            profile.proxy_url.trim().to_string()
        };
        // 只有 Codex 真在跑时才关它（避免运行中的实例覆盖我们写的 config、并让它重启读到新
        // 配置）并等它退干净；没在跑（典型：首次启动 / 切换时已先停掉旧的）就别白等这 500ms。
        if codex_launch::codex_app_process_running() {
            codex_launch::close_codex_app();
            thread::sleep(Duration::from_millis(500));
        }
        // 此刻把 ~/.codex 原样写进固定备份文件（停止/关软件时从这个文件还原）。
        codex_launch::write_launch_backup().map_err(ManagementCoreError::Unavailable)?;

        let mut model_slots = std::collections::BTreeMap::new();
        if !profile.model.trim().is_empty() {
            model_slots.insert(ModelSlot::Sonnet, profile.model.trim().to_string());
        }
        // bearer token（写入 config.toml 的 experimental_bearer_token）：
        // 优先用用户在表单里填的；没填就自动用代理的第一个 api-key，省得手填。
        let api_key = if !profile.api_key.trim().is_empty() {
            profile.api_key.clone()
        } else {
            // 没填就自动取代理的 api-key：先管理快照，再读代理配置兜底。
            self.management_snapshot
                .api_keys
                .first()
                .cloned()
                .or_else(|| get_api_keys().into_iter().next())
                .unwrap_or_default()
        };
        let request = AgentConfigurationRequest {
            agent_id: "codex".to_string(),
            mode: AgentConfigMode::Automatic,
            setup_mode: AgentSetupMode::Proxy,
            storage_option: AgentConfigStorageOption::Json,
            proxy_url,
            api_key,
            model_slots,
            use_oauth: false,
            available_models: Vec::new(),
            reasoning_effort: profile.reasoning.clone(),
        };
        // 直接写代理配置（不刷文件备份）+ 注入选中账号当登录（写 auth.json）。
        agent_config::write_codex_proxy_config_no_backup(&request)?;
        codex_launch::inject_bound_account(&account_key)
            .map_err(ManagementCoreError::Unavailable)?;
        // 刚把 config 切到代理(cliproxyapi)。Codex 只显示 provider 跟当前 config 一致
        // 的历史会话,所以把会话元数据一并对齐到 cliproxyapi,否则历史会话在代理会话里
        // 会消失。此刻 Codex 已关闭(上面 close_codex_app),改库安全。Best-effort。
        let _ = codex_session_visibility::repair_session_visibility_in_default_dir_no_backup();

        let mode = if profile.launch_mode.trim().is_empty() {
            "app"
        } else {
            profile.launch_mode.trim()
        };
        let pid = if mode == "cli" {
            codex_launch::launch_codex_cli().map_err(ManagementCoreError::Unavailable)?
        } else {
            let exe = (!self.settings.codex_app_path.trim().is_empty())
                .then(|| PathBuf::from(self.settings.codex_app_path.trim()))
                .filter(|path| path.exists())
                .or_else(codex_launch::detect_codex_app_path)
                .ok_or_else(|| {
                    ManagementCoreError::Unavailable(
                        "未找到 Codex 应用，请在 Codex 卡片里手填应用路径".to_string(),
                    )
                })?;
            codex_launch::launch_codex_app(&exe).map_err(ManagementCoreError::Unavailable)?
        };
        self.codex_session = Some(codex_launch::CodexSession::new(pid, mode));
        self.codex_session_generation = self.codex_session_generation.wrapping_add(1);
        self.codex_active_profile_id = Some(profile_id.to_string());
        Ok(if mode == "cli" {
            "已在终端启动 Codex（停止会还原配置）".to_string()
        } else {
            match pid {
                Some(pid) => format!("已启动 Codex 应用（pid={pid}）"),
                None => "已启动 Codex 应用".to_string(),
            }
        })
    }

    /// 停止 Codex：杀掉启动的进程 + 还原 auth.json/config.toml 到启动前。
    pub fn codex_stop(&mut self) -> Result<String, ManagementCoreError> {
        let session = self.codex_session.take();
        if session.is_none() && !codex_launch::launch_backup_exists() {
            self.codex_active_profile_id = None;
            return Ok("Codex 未在运行".to_string());
        }

        if let Some(session) = session {
            if let Some(pid) = session.pid {
                codex_launch::kill_process(pid);
            }
        }
        codex_launch::close_codex_app();
        thread::sleep(Duration::from_millis(400));
        codex_launch::restore_codex_state_from_launch_backup()
            .map_err(ManagementCoreError::Unavailable)?;
        // 停了就把绑定账号放回代理池（解除 login-only 占用）。
        self.release_active_profile_binding();
        self.codex_active_profile_id = None;
        Ok("已停止 Codex 并还原配置".to_string())
    }

    /// 当前是否有 Codex 一键启动会话在运行。
    pub fn codex_active(&self) -> bool {
        self.codex_session.is_some() || codex_launch::launch_backup_exists()
    }

    /// quotio-key-router 插件是否就位(管理目录里已有,或安装包内置了待装载)。
    /// 没有它,「按 key 绑定服务商」就不会生成 `plugins:` 路由配置、绑定形同虚设——
    /// 代理仍按全局轮询命中所有可用池。前端据此给「绑了 key 却不隔离」做防呆警告。
    pub fn key_router_plugin_staged(&self) -> bool {
        let dll_name = if cfg!(windows) {
            "quotio-key-router.dll"
        } else if cfg!(target_os = "macos") {
            "quotio-key-router.dylib"
        } else {
            "quotio-key-router.so"
        };
        // 管理目录的代理插件目录(与 ProxyPaths::new 一致:app_config_dir()/proxy)。
        let managed = quotio_platform::app_config_dir()
            .join("proxy")
            .join("plugins")
            .join(dll_name);
        if managed.is_file() {
            return true;
        }
        // 还没装载,但安装包内置了 → 下次写配置时会自动 stage,也算「有」。
        quotio_platform::proxy_resource_dir()
            .join("plugins")
            .join(dll_name)
            .is_file()
    }

    /// 当前在跑的启动方案 id（没有则 None，前端据此高亮「运行中」那套）。
    pub fn active_codex_profile_id(&self) -> Option<String> {
        self.codex_active_profile_id.clone()
    }

    /// 释放当前活动方案绑定账号的 login-only 占用（放回代理池）。best-effort。
    fn release_active_profile_binding(&self) {
        let account = self
            .codex_active_profile_id
            .as_ref()
            .and_then(|id| self.settings.codex_profiles.iter().find(|p| &p.id == id))
            .map(|profile| profile.bound_account.trim().to_string())
            .unwrap_or_default();
        if !account.is_empty() {
            let _ = codex_launch::release_bound_account_login_only(&account);
        }
    }

    /// 监控第一步（持锁，纯内存）：有可监控的会话时返回（会话代数, 探测目标）。
    /// 实际的进程探测（tasklist，几十毫秒）由调用方在锁外执行（[`CodexMonitorProbe::run`]），
    /// 避免拿着 core 锁阻塞 UI 命令。
    pub fn codex_monitor_probe(&self) -> Option<(u64, CodexMonitorProbe)> {
        let session = self.codex_session.as_ref()?;
        let probe = match (session.launch_mode.as_str(), session.pid) {
            // CLI 模式监控终端进程；没拿到终端 pid（cmd start 兜底路径）就无从监控。
            ("cli", Some(pid)) => CodexMonitorProbe::CliByPid(pid),
            ("cli", None) => return None,
            // App 模式按进程名查：启动前已 close_codex_app，跑着的 Codex.exe 都属于本会话。
            _ => CodexMonitorProbe::AppByName,
        };
        Some((self.codex_session_generation, probe))
    }

    /// 监控第二步（持锁，纯内存）：写回锁外的探测结果。用户自己退出 Codex
    /// （没点「停止」）时，自动还原 auth.json/config.toml 并清理会话。
    /// 返回 true 表示发生了自动还原。代数不匹配（探测期间停止/重启过）则丢弃。
    pub fn codex_monitor_apply(&mut self, generation: u64, alive: bool) -> bool {
        if generation != self.codex_session_generation {
            return false;
        }
        let Some(session) = self.codex_session.as_mut() else {
            return false;
        };
        if alive {
            session.seen_running = true;
            session.miss_count = 0;
            return false;
        }
        if session.seen_running {
            // 去抖：连续两次查不到才认定退出，tasklist 偶发失败不触发还原。
            session.miss_count = session.miss_count.saturating_add(1);
            if session.miss_count < 2 {
                return false;
            }
        } else if session.started_at.elapsed() < Duration::from_secs(60) {
            // 启动宽限期：商店版 shell 激活可能要几秒进程才出现。
            // 60 秒还没见到进程就当启动失败，同样还原配置。
            return false;
        }
        self.codex_session = None;
        self.release_active_profile_binding();
        let _ = codex_launch::restore_codex_state_from_launch_backup();
        self.codex_active_profile_id = None;
        true
    }

    /// 拉取代理真实模型所需的参数（推理端点 + 一个 api-key）。
    /// 单独取出来，让命令层在拿到后释放锁再发 HTTP，避免阻塞期间一直持锁。
    pub fn codex_model_fetch_params(&self) -> (String, String) {
        let endpoint = self.proxy.state.endpoint.clone();
        let api_key = self
            .management_snapshot
            .api_keys
            .first()
            .cloned()
            .unwrap_or_default();
        (endpoint, api_key)
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
        let local_accounts = list_local_accounts();
        if management.auth_files.is_empty() {
            management.auth_files = local_accounts;
        } else {
            enrich_auth_files_with_local_markers(&mut management.auth_files, &local_accounts);
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
            logs: self.usage_store.recent_events(500),
            agents: agents::detect_agents(),
            api_keys: api_key_entries(&get_api_keys()),
            api_key_bindings: get_api_key_bindings(),
            request_stats: request_stats_from_management(&self.management_snapshot),
            fallback: self.fallback.clone(),
            fallback_runtime: self.fallback_runtime.clone(),
            credentials: self.credential_status(),
            platform_features: quotio_platform::platform_feature_state(
                self.settings.launch_at_login,
                self.settings.notifications_enabled,
            ),
            config_root: quotio_platform::app_config_dir().display().to_string(),
            scheduler: self.scheduler_status(),
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

fn usage_db_path() -> PathBuf {
    quotio_platform::app_config_dir().join("usage.sqlite")
}

/// Open the on-disk usage store, falling back to an in-memory store (stats won't
/// persist across restarts) if the file can't be opened.
fn open_usage_store() -> Arc<UsageStore> {
    match UsageStore::open(&usage_db_path()) {
        Ok(store) => Arc::new(store),
        Err(_) => Arc::new(UsageStore::open_in_memory()),
    }
}

/// Test-only redirect for [`api_keys_path`]. Management-snapshot tests point this
/// at a seeded temp fixture so assertions read the fixture key instead of the
/// developer's real on-disk `api-keys.json`. Mirrors `PROXY_RESOURCE_ROOT` in
/// quotio-platform, but `#[cfg(test)]`-gated so it is compiled out of release builds.
#[cfg(test)]
static API_KEYS_PATH_OVERRIDE: std::sync::OnceLock<std::sync::RwLock<Option<PathBuf>>> =
    std::sync::OnceLock::new();

#[cfg(test)]
fn set_api_keys_path_override(path: Option<PathBuf>) {
    let lock = API_KEYS_PATH_OVERRIDE.get_or_init(|| std::sync::RwLock::new(None));
    *lock.write().expect("api-keys path override lock poisoned") = path;
}

fn api_keys_path() -> PathBuf {
    #[cfg(test)]
    {
        if let Some(lock) = API_KEYS_PATH_OVERRIDE.get() {
            if let Some(path) = lock
                .read()
                .expect("api-keys path override lock poisoned")
                .clone()
            {
                return path;
            }
        }
    }
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

// ── API Key → Provider binding ──────────────────────────────────────────

fn api_key_bindings_path() -> PathBuf {
    quotio_platform::app_config_dir().join("api-key-bindings.json")
}

pub fn get_api_key_bindings() -> Vec<ApiKeyBinding> {
    std::fs::read_to_string(api_key_bindings_path())
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

fn save_api_key_bindings(bindings: &[ApiKeyBinding]) -> Result<(), String> {
    let path = api_key_bindings_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let body = serde_json::to_string_pretty(bindings).map_err(|e| e.to_string())?;
    std::fs::write(&path, body).map_err(|e| e.to_string())
}

pub fn set_api_key_binding(api_key: String, provider_id: String) -> Result<Vec<ApiKeyBinding>, String> {
    let mut bindings = get_api_key_bindings();
    bindings.retain(|b| b.api_key != api_key);
    if !provider_id.is_empty() {
        bindings.push(ApiKeyBinding {
            api_key,
            provider_id,
        });
    }
    save_api_key_bindings(&bindings)?;
    Ok(bindings)
}

pub fn remove_api_key_binding(api_key: &str) -> Result<Vec<ApiKeyBinding>, String> {
    let mut bindings = get_api_key_bindings();
    bindings.retain(|b| b.api_key != api_key);
    save_api_key_bindings(&bindings)?;
    Ok(bindings)
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
        .map(|key| ApiKeyEntry {
            value: key.clone(),
            masked_value: mask_secret(key),
            source: "management".to_string(),
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
/// Extract the account email from a parsed CLIProxyAPI credential JSON.
/// All JWT tokens a credential file carries — checks both the flat CPA format
/// (top-level `id_token`/`access_token`) and the raw Codex format (nested under
/// `tokens`).
fn credential_tokens(value: &serde_json::Value) -> Vec<String> {
    let mut tokens = Vec::new();
    for key in ["id_token", "access_token"] {
        if let Some(token) = value.get(key).and_then(|v| v.as_str()) {
            if !token.is_empty() {
                tokens.push(token.to_string());
            }
        }
        if let Some(token) = value
            .get("tokens")
            .and_then(|t| t.get(key))
            .and_then(|v| v.as_str())
        {
            if !token.is_empty() {
                tokens.push(token.to_string());
            }
        }
    }
    tokens
}

/// Stable account identity (ChatGPT `account_id`) for de-dup: top-level/nested
/// `account_id`, else the `chatgpt_account_id` claim inside any carried JWT — so
/// it recognizes the same account regardless of file format (flat CPA export or a
/// raw Codex `auth.json` whose identity only lives in the token).
fn credential_account_id(value: &serde_json::Value) -> Option<String> {
    for direct in [
        value.get("account_id"),
        value.get("tokens").and_then(|t| t.get("account_id")),
    ] {
        if let Some(id) = direct.and_then(|v| v.as_str()).filter(|s| !s.is_empty()) {
            return Some(id.to_string());
        }
    }
    for token in credential_tokens(value) {
        if let Some(id) = crate::quota::decode_jwt_payload(&token).and_then(|payload| {
            payload
                .get("https://api.openai.com/auth")
                .and_then(|auth| auth.get("chatgpt_account_id"))
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(str::to_string)
        }) {
            return Some(id);
        }
    }
    None
}

/// Account email: top-level/nested `email`, else the email claim inside a carried
/// JWT (handles files that don't store a top-level email).
fn credential_email(value: &serde_json::Value) -> Option<String> {
    for direct in [
        value.get("email"),
        value.get("tokens").and_then(|t| t.get("email")),
    ] {
        if let Some(email) = direct.and_then(|v| v.as_str()).filter(|s| !s.is_empty()) {
            return Some(email.to_string());
        }
    }
    for token in credential_tokens(value) {
        let Some(payload) = crate::quota::decode_jwt_payload(&token) else {
            continue;
        };
        let claim = payload
            .get("email")
            .and_then(|v| v.as_str())
            .or_else(|| {
                payload
                    .get("https://api.openai.com/profile")
                    .and_then(|p| p.get("email"))
                    .and_then(|v| v.as_str())
            })
            .filter(|s| !s.is_empty());
        if let Some(email) = claim {
            return Some(email.to_string());
        }
    }
    None
}

/// Extract the provider/type declared inside a credential JSON (reads the file's
/// own `type`/`provider` field, ignoring the filename so renamed files still
/// compare correctly).
fn credential_provider(value: &serde_json::Value) -> Option<String> {
    value
        .get("type")
        .or_else(|| value.get("provider"))
        .and_then(|kind| kind.as_str())
        .map(str::to_string)
        .filter(|kind| !kind.is_empty())
}

/// Remove duplicate credential files that point at the SAME login (by
/// `account_id`+email, so ChatGPT Team members — who share one account_id but
/// are separate seats — are NOT merged), keeping ONE per login: the bound-login
/// file if the group has one (its filename is referenced by settings, so deleting
/// it would break Codex launch), otherwise the most-recently-modified file. Skips
/// `glm-keys*` and non-JSON. Best-effort — fixes "re-import / re-login of the same
/// member shows two cards" without touching distinct logins/accounts.
/// Detect & convert a `sub2api-data` export into CLIProxyAPI auth objects.
///
/// sub2api nests each account's OAuth tokens under `credentials` and wraps them
/// in `{ type:"sub2api-data", accounts:[…] }`. We flatten the credentials to the
/// top level and rename to the codex-auth shape (`chatgpt_account_id`→`account_id`,
/// `expires_at`→`expired`), so the normal importer can write `codex-<email>.json`.
/// Only `platform:"openai"` → `codex` is supported for now; other platforms are
/// skipped. Returns `None` when the JSON isn't a sub2api export (so the caller
/// falls back to the existing array/object handling).
fn convert_sub2api_accounts(parsed: &serde_json::Value) -> Option<Vec<serde_json::Value>> {
    let obj = parsed.as_object()?;
    if obj.get("type").and_then(|v| v.as_str()) != Some("sub2api-data") {
        return None;
    }
    let accounts = obj.get("accounts")?.as_array()?;
    let mut out = Vec::new();
    for account in accounts {
        let Some(account) = account.as_object() else {
            continue;
        };
        let provider = match account.get("platform").and_then(|v| v.as_str()) {
            Some("openai") => "codex",
            _ => continue, // 暂只支持 openai → codex,其它平台跳过
        };
        let Some(creds) = account.get("credentials").and_then(|v| v.as_object()) else {
            continue;
        };
        // 至少要有 email(做文件名)和 access_token(可用)才转,否则跳过避免写废文件。
        let has = |key: &str| {
            creds
                .get(key)
                .and_then(|v| v.as_str())
                .is_some_and(|s| !s.is_empty())
        };
        if !has("email") || !has("access_token") {
            continue;
        }
        let mut auth = serde_json::Map::new();
        auth.insert("type".to_string(), serde_json::Value::String(provider.to_string()));
        for key in ["access_token", "refresh_token", "id_token", "email"] {
            if let Some(value) = creds.get(key) {
                auth.insert(key.to_string(), value.clone());
            }
        }
        if let Some(value) = creds.get("chatgpt_account_id") {
            auth.insert("account_id".to_string(), value.clone());
        }
        if let Some(value) = creds.get("expires_at") {
            auth.insert("expired".to_string(), value.clone());
        }
        auth.insert("disabled".to_string(), serde_json::Value::Bool(false));
        out.push(serde_json::Value::Object(auth));
    }
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

/// Detect & convert an `accounts-export.json` 导出 into CLIProxyAPI codex auth objects.
///
/// 该格式形如 `{ accounts: [{ token, refreshToken, email, accountId, ... }] }`:每个账号的 OAuth
/// 令牌**扁平、驼峰命名**(`token`=access、`refreshToken`=refresh、`accountId`=account_id),且
/// **没有 `id_token`**。这里把它们重命名成 codex-auth 结构;`id_token` 写空串占位(满足
/// `is_codex_auth` 的存在性检查),CLIProxyAPI 首次请求会用 `refresh_token` 续期并补全。
/// 不是这个形状就返回 `None`(交回上层的 sub2api / 数组 / 单对象处理)。
fn convert_accounts_export(parsed: &serde_json::Value) -> Option<Vec<serde_json::Value>> {
    let obj = parsed.as_object()?;
    // sub2api 有自己的 type 标记,让它自己的转换器先处理。
    if obj.get("type").and_then(|v| v.as_str()) == Some("sub2api-data") {
        return None;
    }
    let accounts = obj.get("accounts")?.as_array()?;
    let mut out = Vec::new();
    for account in accounts {
        let Some(account) = account.as_object() else {
            continue;
        };
        let field = |key: &str| {
            account
                .get(key)
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
        };
        // 该格式特征 + 最低可用要求:access(token)+ refreshToken,且至少有 email / accountId 做文件名。
        let (Some(access), Some(refresh)) = (field("token"), field("refreshToken")) else {
            continue;
        };
        if field("email").is_none() && field("accountId").is_none() {
            continue;
        }
        let mut auth = serde_json::Map::new();
        auth.insert("type".to_string(), serde_json::Value::String("codex".to_string()));
        auth.insert(
            "access_token".to_string(),
            serde_json::Value::String(access.to_string()),
        );
        auth.insert(
            "refresh_token".to_string(),
            serde_json::Value::String(refresh.to_string()),
        );
        // 导出不带 id_token —— 空串占位,首次续期补全。
        auth.insert("id_token".to_string(), serde_json::Value::String(String::new()));
        if let Some(email) = field("email") {
            auth.insert("email".to_string(), serde_json::Value::String(email.to_string()));
        }
        if let Some(account_id) = field("accountId") {
            auth.insert(
                "account_id".to_string(),
                serde_json::Value::String(account_id.to_string()),
            );
        }
        auth.insert("disabled".to_string(), serde_json::Value::Bool(false));
        out.push(serde_json::Value::Object(auth));
    }
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

/// 给导入的账号文件取一个稳定且唯一的标识段(用于文件名 `<provider>-<ident>.json`):
/// 优先 email,其次 account_id(顶层或 tokens 里),都没有就用内容指纹。关键是别让多份
/// 「没 email」的账号都落到同一个 `<provider>-unknown.json` 而互相覆盖(= 多选导入只进来一个)。
fn auth_file_ident(item: &serde_json::Value) -> String {
    fn sanitize(s: &str) -> String {
        s.chars()
            .map(|c| {
                if c.is_alphanumeric() || c == '@' || c == '.' || c == '-' || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect()
    }
    let obj = item.as_object();
    let email = obj
        .and_then(|o| o.get("email"))
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty());
    if let Some(email) = email {
        return sanitize(email);
    }
    let account_id = obj
        .and_then(|o| o.get("account_id"))
        .and_then(|v| v.as_str())
        .or_else(|| {
            obj.and_then(|o| o.get("tokens"))
                .and_then(|tokens| tokens.get("account_id"))
                .and_then(|v| v.as_str())
        })
        .filter(|s| !s.is_empty());
    if let Some(account_id) = account_id {
        return format!("aid-{}", sanitize(account_id));
    }
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    std::hash::Hash::hash(&item.to_string(), &mut hasher);
    format!("anon-{:x}", std::hash::Hasher::finish(&hasher))
}

fn dedup_codex_auth_keep_newest(dir: &std::path::Path, bound_account: &str) {
    use std::collections::HashMap;
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let bound = bound_account.trim();
    let mut groups: HashMap<String, Vec<(PathBuf, std::time::SystemTime, bool)>> = HashMap::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|v| v.to_str()) else {
            continue;
        };
        let lower = name.to_ascii_lowercase();
        if !lower.ends_with(".json") || lower.starts_with("glm-keys") {
            continue;
        }
        let Some(value) = std::fs::read_to_string(&path)
            .ok()
            .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok())
        else {
            continue;
        };
        // Identity = member email + provider. ChatGPT Team members are SEPARATE
        // seats that share ONE account_id but each has its OWN email, so keying on
        // email keeps members distinct (a Team's members never collide) while a
        // re-import of the SAME member (same email) collapses — INCLUDING a dead or
        // stale copy whose token can't be parsed. The previous account_id+email key
        // failed exactly there: a broken copy with no parseable account_id fell back
        // to an email-only key and never matched its healthy twin (keyed on
        // account_id+email), so duplicates piled up (the reported "导入 Team 显示重复
        // 账号"). Provider keeps the same email on two providers (codex vs gemini)
        // distinct. Fall back to account_id only when there is NO email; no identity
        // at all → leave the file alone.
        let provider = credential_provider(&value).unwrap_or_default().to_ascii_lowercase();
        let identity = match credential_email(&value) {
            Some(email) => format!("{}|{}", email.to_ascii_lowercase(), provider),
            None => match credential_account_id(&value) {
                Some(id) => format!("aid:{}|{}", id.to_ascii_lowercase(), provider),
                None => continue,
            },
        };
        let stem = name.strip_suffix(".json").unwrap_or(name);
        let is_bound = !bound.is_empty() && (name == bound || stem == bound);
        let mtime = entry
            .metadata()
            .and_then(|meta| meta.modified())
            .unwrap_or(std::time::UNIX_EPOCH);
        groups
            .entry(identity)
            .or_default()
            .push((path, mtime, is_bound));
    }
    for files in groups.into_values() {
        if files.len() < 2 {
            continue;
        }
        // Keep the bound file if present, else the newest; remove the rest.
        let keep = files.iter().position(|f| f.2).unwrap_or_else(|| {
            let mut best = 0usize;
            for (index, file) in files.iter().enumerate() {
                if file.1 > files[best].1 {
                    best = index;
                }
            }
            best
        });
        for (index, file) in files.iter().enumerate() {
            if index != keep {
                let _ = std::fs::remove_file(&file.0);
            }
        }
    }
}

/// Append failed requests (HTTP >= 400, or flagged failed) to a per-day JSONL
/// error log under the app logs dir, then prune logs older than 30 days. Best
/// effort — never fails the caller. Gives users a `errors-YYYY-MM-DD.jsonl` they
/// can hand over for diagnosis. Includes the redacted raw usage snapshot so any
/// upstream error detail the proxy reported is captured too.
pub fn append_request_errors(events: &[quotio_types::UsageEvent]) {
    use std::io::Write as _;
    let failed: Vec<&quotio_types::UsageEvent> = events
        .iter()
        .filter(|event| event.failed || event.status_code.is_some_and(|status| status >= 400))
        .collect();
    if failed.is_empty() {
        return;
    }
    let dir = quotio_platform::app_logs_dir();
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    prune_old_error_logs(&dir);

    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let path = dir.join(format!("errors-{today}.jsonl"));
    let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(&path) else {
        return;
    };
    for event in failed {
        let line = serde_json::json!({
            "ts": event.timestamp,
            "account": event.source,
            "model": event.model,
            "requested_model": event.requested_model,
            "reasoning_effort": event.reasoning_effort,
            "provider": event.provider,
            "endpoint": event.endpoint,
            "status": event.status_code,
            "latency_ms": event.latency_ms,
            "raw": event.raw_json,
        });
        let _ = writeln!(file, "{line}");
    }
}

/// Delete `errors-YYYY-MM-DD.jsonl` files whose date is more than 30 days old.
/// `YYYY-MM-DD` strings compare in chronological order, so a plain `<` works.
fn prune_old_error_logs(dir: &std::path::Path) {
    let cutoff = (chrono::Local::now() - chrono::Duration::days(30))
        .format("%Y-%m-%d")
        .to_string();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if let Some(date) = name
            .strip_prefix("errors-")
            .and_then(|rest| rest.strip_suffix(".jsonl"))
        {
            if date < cutoff.as_str() {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
}

/// Open the app logs dir (where `errors-*.jsonl` live) in the OS file manager,
/// creating it first so the action works even before any error was logged.
pub fn open_logs_dir() -> Result<(), String> {
    let dir = quotio_platform::app_logs_dir();
    let _ = std::fs::create_dir_all(&dir);
    quotio_platform::open_file_manager(&dir).map_err(|error| format!("无法打开日志目录：{error}"))
}

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
        let disabled = parsed
            .as_ref()
            .and_then(|value| value.get("disabled"))
            .and_then(|value| value.as_bool())
            .unwrap_or(false);
        let quotio_bound_login_only = parsed
            .as_ref()
            .and_then(|value| value.get("quotio_bound_login_only"))
            .and_then(|value| value.as_bool());
        let quotio_scheduler_standby = parsed
            .as_ref()
            .and_then(|value| value.get("quotio_scheduler_standby"))
            .and_then(|value| value.as_bool());
        files.push(AuthFile {
            id: name.to_string(),
            name: name.to_string(),
            provider,
            label: None,
            status: "local".to_string(),
            status_message: None,
            disabled,
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
            quotio_bound_login_only,
            quotio_scheduler_standby,
            success: None,
            failed: None,
            recent_requests: None,
        });
    }
    files.sort_by(|left, right| left.name.cmp(&right.name));
    files
}

/// Bundle every CPA account credential file (`~/.cli-proxy-api/*.json`, minus the
/// GLM key file) into a single zip under `dest_dir`, for backup / moving to
/// another machine. Returns the written zip's path; errors when there's nothing
/// to export. NOTE: the zip contains live OAuth tokens — treat it as a secret.
pub fn export_auth_files(
    zip_path: &std::path::Path,
    names: Option<&[String]>,
) -> Result<String, String> {
    use std::io::Write as _;
    // names = Some 时只导这些账号(按服务商导出);None = 导全部。
    // 归一化(去 .json 后缀 + 小写)后比对,兼容代理把文件名小写化、以及传入带不带 .json。
    let normalize = |value: &str| value.trim().trim_end_matches(".json").to_ascii_lowercase();
    let only: Option<std::collections::HashSet<String>> =
        names.map(|list| list.iter().map(|name| normalize(name)).collect());
    let src = quotio_platform::proxy_auth_dir();
    let entries = std::fs::read_dir(&src).map_err(|error| format!("读取账号目录失败：{}", error))?;
    if let Some(parent) = zip_path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| format!("创建导出目录失败：{}", error))?;
    }
    let file =
        std::fs::File::create(zip_path).map_err(|error| format!("创建导出文件失败：{}", error))?;
    let mut zip = zip::ZipWriter::new(file);
    let options =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let mut count = 0usize;
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        let lower = name.to_ascii_lowercase();
        if !lower.ends_with(".json") || lower.starts_with("glm-keys") {
            continue;
        }
        if let Some(only) = &only {
            if !only.contains(&normalize(name)) {
                continue;
            }
        }
        let Ok(content) = std::fs::read(&path) else {
            continue;
        };
        zip.start_file(name, options)
            .map_err(|error| format!("写入压缩包失败：{}", error))?;
        zip.write_all(&content)
            .map_err(|error| format!("写入压缩包失败：{}", error))?;
        count += 1;
    }
    zip.finish().map_err(|error| format!("完成压缩包失败：{}", error))?;
    if count == 0 {
        let _ = std::fs::remove_file(zip_path);
        return Err("没有可导出的账号文件".to_string());
    }
    Ok(zip_path.display().to_string())
}

fn enrich_auth_files_with_local_markers(files: &mut [AuthFile], local_accounts: &[AuthFile]) {
    for file in files {
        // Case-INSENSITIVE match: the proxy's /auth-files lowercases filenames
        // while the local auth dir keeps the original case, so an exact `==`
        // would skip enrichment for any mixed-case file (e.g. the scheduler
        // standby marker), making a parked account look user-disabled.
        let Some(local) = local_accounts
            .iter()
            .find(|local| local.name.eq_ignore_ascii_case(&file.name))
        else {
            continue;
        };
        if file.quotio_bound_login_only.is_none() {
            file.quotio_bound_login_only = local.quotio_bound_login_only;
        }
        if file.quotio_scheduler_standby.is_none() {
            file.quotio_scheduler_standby = local.quotio_scheduler_standby;
        }
        if local.quotio_bound_login_only == Some(true) {
            file.disabled = true;
        }
    }
}

/// A single API key inside a custom provider's key pool.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProviderKey {
    pub id: String,
    #[serde(default)]
    pub label: String,
    pub api_key: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_weight")]
    pub weight: u32,
}

fn default_true() -> bool { true }
fn default_weight() -> u32 { 1 }

/// A user-defined third-party provider (OpenAI/Gemini-compatible endpoint).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CustomProvider {
    pub id: String,
    pub name: String,
    pub base_url: String,
    /// Legacy single key — kept for backward compat. On load, migrated to `keys`.
    #[serde(default)]
    pub api_key: String,
    pub kind: String,
    #[serde(default)]
    pub prefix: String,
    /// Key pool. If empty on load but `api_key` is set, auto-migrated.
    #[serde(default)]
    pub keys: Vec<ProviderKey>,
    #[serde(default)]
    pub default_model: String,
    /// Models this provider serves. REQUIRED for routing: CLIProxyAPI registers
    /// zero models for an api-key provider with an empty list, making it
    /// unroutable (no candidate for any model request). User-entered.
    #[serde(default)]
    pub models: Vec<String>,
    /// How this provider reaches its upstream. "" / "inherit" → use the global
    /// proxy-url (settings proxy). "direct" → bypass it (emits per-entry
    /// `proxy-url: 'direct'`, CLIProxyAPI ModeDirect). Lets a domestic interface
    /// (e.g. anyrouter) go direct while OpenAI/Anthropic still use the proxy.
    #[serde(default)]
    pub proxy_mode: String,
}

/// Parse a user-entered model list (comma / whitespace / newline separated) into
/// a trimmed, deduped Vec. Model ids never contain whitespace, so splitting on
/// any of those separators is safe.
fn parse_model_list(raw: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for token in raw.split(|c: char| c == ',' || c.is_whitespace()) {
        let m = token.trim();
        if !m.is_empty() && !out.iter().any(|existing| existing == m) {
            out.push(m.to_string());
        }
    }
    out
}

/// Normalize the per-provider connection mode. "direct"/"none" → "direct"
/// (bypass the global proxy); anything else → "" (inherit the global proxy).
fn normalize_proxy_mode(raw: &str) -> String {
    match raw.trim().to_ascii_lowercase().as_str() {
        "direct" | "none" => "direct".to_string(),
        _ => String::new(),
    }
}

fn custom_providers_path() -> PathBuf {
    quotio_platform::app_config_dir().join("custom-providers.json")
}

pub fn list_custom_providers() -> Vec<CustomProvider> {
    let mut list: Vec<CustomProvider> = std::fs::read_to_string(custom_providers_path())
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default();
    // Migrate legacy single api_key → keys pool.
    let mut migrated = false;
    for provider in &mut list {
        if provider.keys.is_empty() && !provider.api_key.is_empty() {
            provider.keys.push(ProviderKey {
                id: Uuid::new_v4().to_string(),
                label: "默认".to_string(),
                api_key: provider.api_key.clone(),
                enabled: true,
                weight: 1,
            });
            provider.api_key.clear();
            migrated = true;
        }
    }
    if migrated {
        let _ = save_custom_providers(&list);
    }
    list
}

fn save_custom_providers(list: &[CustomProvider]) -> Result<(), String> {
    let path = custom_providers_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| format!("创建配置目录失败：{}", error))?;
    }
    let json =
        serde_json::to_string_pretty(list).map_err(|error| format!("序列化失败：{}", error))?;
    std::fs::write(&path, json).map_err(|error| format!("写入自定义服务商失败：{}", error))
}

pub fn add_custom_provider(
    name: String,
    base_url: String,
    api_key: String,
    kind: String,
    prefix: String,
    models: String,
    proxy_mode: String,
) -> Result<Vec<CustomProvider>, String> {
    let name = name.trim();
    let base_url = base_url.trim();
    if name.is_empty() || base_url.is_empty() {
        return Err("名称和 Base URL 必填。".to_string());
    }
    let kind = kind.trim();
    let trimmed_key = api_key.trim();
    let keys = if trimmed_key.is_empty() {
        vec![]
    } else {
        vec![ProviderKey {
            id: Uuid::new_v4().to_string(),
            label: "默认".to_string(),
            api_key: trimmed_key.to_string(),
            enabled: true,
            weight: 1,
        }]
    };
    let mut list = list_custom_providers();
    list.push(CustomProvider {
        id: Uuid::new_v4().to_string(),
        name: name.to_string(),
        base_url: base_url.to_string(),
        api_key: String::new(),
        kind: if kind.is_empty() {
            "openai".to_string()
        } else {
            kind.to_string()
        },
        prefix: prefix.trim().to_string(),
        keys,
        default_model: String::new(),
        models: parse_model_list(&models),
        proxy_mode: normalize_proxy_mode(&proxy_mode),
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
    models: String,
    proxy_mode: String,
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
    // Don't overwrite key pool with legacy api_key — only set if pool is empty
    if provider.keys.is_empty() && !api_key.trim().is_empty() {
        provider.keys.push(ProviderKey {
            id: Uuid::new_v4().to_string(),
            label: "默认".to_string(),
            api_key: api_key.trim().to_string(),
            enabled: true,
            weight: 1,
        });
    }
    provider.api_key.clear();
    provider.kind = if kind.is_empty() {
        "openai".to_string()
    } else {
        kind.to_string()
    };
    provider.prefix = prefix.trim().to_string();
    provider.models = parse_model_list(&models);
    provider.proxy_mode = normalize_proxy_mode(&proxy_mode);
    save_custom_providers(&list)?;
    Ok(list)
}

pub fn add_provider_key(
    provider_id: &str,
    label: String,
    api_key: String,
) -> Result<Vec<CustomProvider>, String> {
    let api_key = api_key.trim();
    if api_key.is_empty() {
        return Err("API Key 不能为空。".to_string());
    }
    let mut list = list_custom_providers();
    let Some(provider) = list.iter_mut().find(|p| p.id == provider_id) else {
        return Err("未找到服务商。".to_string());
    };
    provider.keys.push(ProviderKey {
        id: Uuid::new_v4().to_string(),
        label: label.trim().to_string(),
        api_key: api_key.to_string(),
        enabled: true,
        weight: 1,
    });
    save_custom_providers(&list)?;
    Ok(list)
}

pub fn remove_provider_key(
    provider_id: &str,
    key_id: &str,
) -> Result<Vec<CustomProvider>, String> {
    let mut list = list_custom_providers();
    let Some(provider) = list.iter_mut().find(|p| p.id == provider_id) else {
        return Err("未找到服务商。".to_string());
    };
    provider.keys.retain(|k| k.id != key_id);
    save_custom_providers(&list)?;
    Ok(list)
}

pub fn toggle_provider_key(
    provider_id: &str,
    key_id: &str,
) -> Result<Vec<CustomProvider>, String> {
    let mut list = list_custom_providers();
    let Some(provider) = list.iter_mut().find(|p| p.id == provider_id) else {
        return Err("未找到服务商。".to_string());
    };
    let Some(key) = provider.keys.iter_mut().find(|k| k.id == key_id) else {
        return Err("未找到该密钥。".to_string());
    };
    key.enabled = !key.enabled;
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
    let bindings = get_api_key_bindings();
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
            let enabled_keys: Vec<&ProviderKey> =
                provider.keys.iter().filter(|k| k.enabled).collect();
            let bound_client_keys: Vec<&str> = bindings
                .iter()
                .filter(|b| b.provider_id == provider.id)
                .map(|b| b.api_key.as_str())
                .collect();
            if section == "openai-compatibility" {
                out.push_str(&format!("  - name: {}\n", yaml_quote(&provider.name)));
                out.push_str(&format!("    base-url: {}\n", yaml_quote(&provider.base_url)));
                if !provider.prefix.is_empty() {
                    out.push_str(&format!("    prefix: {}\n", yaml_quote(&provider.prefix)));
                }
                // REQUIRED for routing: an empty models list registers zero models
                // (CLIProxyAPI buildOpenAICompatibilityConfigModels → nil), leaving
                // the provider unroutable — no candidate for any model request.
                if !provider.models.is_empty() {
                    out.push_str("    models:\n");
                    for model in &provider.models {
                        out.push_str(&format!("      - name: {}\n", yaml_quote(model)));
                    }
                }
                if !bound_client_keys.is_empty() {
                    out.push_str("    allowed-api-keys:\n");
                    for client_key in &bound_client_keys {
                        out.push_str(&format!("      - {}\n", yaml_quote(client_key)));
                    }
                }
                if !enabled_keys.is_empty() {
                    out.push_str("    api-key-entries:\n");
                    for key in &enabled_keys {
                        out.push_str(&format!("      - api-key: {}\n", yaml_quote(&key.api_key)));
                        // "direct" bypasses the global proxy-url for this upstream
                        // (CLIProxyAPI ModeDirect); empty inherits the global proxy.
                        if provider.proxy_mode == "direct" {
                            out.push_str("        proxy-url: 'direct'\n");
                        }
                    }
                }
            } else {
                // Non-openai types: each enabled key becomes a separate entry.
                for key in &enabled_keys {
                    out.push_str(&format!("  - api-key: {}\n", yaml_quote(&key.api_key)));
                    if !provider.base_url.is_empty() {
                        out.push_str(&format!("    base-url: {}\n", yaml_quote(&provider.base_url)));
                    }
                    // "direct" bypasses the global proxy-url for this upstream.
                    if provider.proxy_mode == "direct" {
                        out.push_str("    proxy-url: 'direct'\n");
                    }
                    if !provider.prefix.is_empty() {
                        out.push_str(&format!("    prefix: {}\n", yaml_quote(&provider.prefix)));
                    }
                    // REQUIRED for routing — empty models ⇒ zero registered models
                    // ⇒ unroutable (same as the openai-compatibility branch).
                    if !provider.models.is_empty() {
                        out.push_str("    models:\n");
                        for model in &provider.models {
                            out.push_str(&format!("      - name: {}\n", yaml_quote(model)));
                        }
                    }
                    if !bound_client_keys.is_empty() {
                        out.push_str("    allowed-api-keys:\n");
                        for client_key in &bound_client_keys {
                            out.push_str(&format!("      - {}\n", yaml_quote(client_key)));
                        }
                    }
                }
            }
        }
    }
    out
}

/// Plugin id of the per-key router — must equal its dll filename stem, which is
/// how CLIProxyAPI derives the id it matches against `plugins.configs.<id>`.
const KEY_ROUTER_PLUGIN_ID: &str = "quotio-key-router";

/// Host (+ port) of a base-url, used to match a custom pool's candidate by its
/// `base_url` attribute. `https://anyrouter.top/v1` → `anyrouter.top`.
fn base_url_host(base_url: &str) -> String {
    base_url
        .trim()
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .split('/')
        .next()
        .unwrap_or(base_url)
        .to_string()
}

/// Render the `plugins:` section that loads the quotio-key-router scheduler
/// plugin and maps each bound api-key to the pool it's allowed to use. Empty when
/// the plugin binary isn't staged or no api-keys are bound. `strict: true` denies
/// any unmapped key — the secure posture for an exposed/shared proxy link.
fn key_router_plugins_yaml(proxy_dir: &Path) -> String {
    let plugins_dir = proxy_dir.join("plugins");
    let dll_name = if cfg!(windows) {
        "quotio-key-router.dll"
    } else if cfg!(target_os = "macos") {
        "quotio-key-router.dylib"
    } else {
        "quotio-key-router.so"
    };
    let managed_dll = plugins_dir.join(dll_name);
    // (Re)stage the bundled plugin whenever it differs (by size) from the managed
    // copy: a fresh install that ships it under resources/proxy/<plat>/plugins/
    // gets it without manual placement, AND an app upgrade shipping a newer plugin
    // replaces the old one instead of the stale managed copy shadowing it. Mirrors
    // kiro_sidecar::resolve_binary and the proxy core.
    let bundled_dll = quotio_platform::proxy_resource_dir()
        .join("plugins")
        .join(dll_name);
    if bundled_dll.is_file() {
        let managed_len = std::fs::metadata(&managed_dll).map(|m| m.len()).ok();
        let bundled_len = std::fs::metadata(&bundled_dll).map(|m| m.len()).ok();
        if managed_len != bundled_len {
            let _ = std::fs::create_dir_all(&plugins_dir);
            let _ = std::fs::copy(&bundled_dll, &managed_dll);
        }
    }
    if !managed_dll.is_file() {
        return String::new();
    }
    let bindings = get_api_key_bindings();
    if bindings.is_empty() {
        return String::new();
    }

    let customs = list_custom_providers();
    let mut routes = String::new();
    for binding in &bindings {
        let key = yaml_quote(&binding.api_key);
        if binding.provider_id == kiro_sidecar::KIRO_PROVIDER_ID {
            // Kiro is served by a claude-api-key provider (CLIProxyAPI Provider
            // "claude") that points at the kiro-rs sidecar — pin it by base-url so
            // it's distinct from a real Anthropic pool.
            routes.push_str(&format!(
                "        - key: {key}\n          provider: 'claude'\n          base_url_contains: '{}'\n",
                kiro_sidecar::KIRO_SIDECAR_PORT
            ));
        } else if let Some(custom) = customs.iter().find(|c| c.id == binding.provider_id) {
            // A custom provider's id is an internal UUID, but CLIProxyAPI tags its
            // candidate by TYPE (e.g. "openai"), not the UUID — so match on the
            // base-url host instead, which uniquely identifies this pool.
            routes.push_str(&format!(
                "        - key: {key}\n          base_url_contains: {}\n",
                yaml_quote(&base_url_host(&custom.base_url))
            ));
        } else {
            // Built-in pools: Quotio's provider_id equals the CLIProxyAPI provider
            // type (codex, gemini, …).
            routes.push_str(&format!(
                "        - key: {key}\n          provider: {}\n",
                yaml_quote(&binding.provider_id)
            ));
        }
    }

    format!(
        "\n# Per-key pool gating (managed by Quotio — quotio-key-router plugin)\n\
         plugins:\n  enabled: true\n  dir: {}\n  configs:\n    {}:\n      \
         enabled: true\n      priority: 1\n      strict: true\n      routes:\n{}",
        yaml_quote(&plugins_dir.display().to_string()),
        KEY_ROUTER_PLUGIN_ID,
        routes
    )
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
    parse_settings_tolerant(&content)
}

/// 容错解析设置。直接 `from_str` 是「全有或全无」:只要某个字段格式跨版本变了
/// (典型是枚举变体改名),serde 会拒绝整份 JSON,上层 `unwrap_or_default()` 就把
/// 用户全部设置重置成默认、并在启动时写回覆盖——这正是升级后「配置被初始化」的根因。
///
/// 这里改成:先走快路径(整份能解析就直接用);否则把用户的每个字段逐个盖到默认值
/// 上,只丢弃放不进当前 schema 的字段,其余(代理、端口、语言、主题、调度规则……)
/// 原样保留。仅在罕见的格式不兼容时走慢路径,启动时跑一次,开销可忽略。
fn parse_settings_tolerant(content: &str) -> Option<AppSettings> {
    // 快路径:当前版本能完整解析。
    if let Ok(settings) = serde_json::from_str::<AppSettings>(content) {
        return Some(settings);
    }
    // 慢路径:逐字段抢救。
    let user: serde_json::Value = serde_json::from_str(content).ok()?;
    let user_obj = user.as_object()?;
    let mut merged = serde_json::to_value(AppSettings::default()).ok()?;
    let merged_obj = merged.as_object_mut()?;
    for (key, value) in user_obj {
        let previous = merged_obj.get(key).cloned();
        merged_obj.insert(key.clone(), value.clone());
        // 放进去后整体还能解析就保留;否则这个字段不兼容,回退默认。
        if serde_json::from_value::<AppSettings>(serde_json::Value::Object(merged_obj.clone()))
            .is_err()
        {
            match previous {
                Some(prev) => merged_obj.insert(key.clone(), prev),
                None => merged_obj.remove(key),
            };
        }
    }
    serde_json::from_value(merged).ok()
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

/// 把旧的单套平铺 Codex 启动配置迁移成一条方案。仅在还没有任何方案、且配过绑定账号时
/// 跑一次：按旧字段合成 profiles[0]，并清空旧平铺字段（单一数据源——避免用户删光方案后
/// 下次启动又被旧字段「复活」出一条）。迁移结果在 `Default` 里随其它设置一并写回磁盘。
fn migrate_codex_profiles(settings: &mut AppSettings) {
    if !settings.codex_profiles.is_empty() {
        return;
    }
    let account = settings.codex_bound_account.trim().to_string();
    if account.is_empty() {
        return;
    }
    let launch_mode = if settings.codex_launch_mode.trim().is_empty() {
        "app".to_string()
    } else {
        settings.codex_launch_mode.trim().to_string()
    };
    let reasoning = if settings.codex_reasoning.trim().is_empty() {
        "high".to_string()
    } else {
        settings.codex_reasoning.trim().to_string()
    };
    settings.codex_profiles.push(quotio_types::CodexLaunchProfile {
        id: "migrated-default".to_string(),
        name: profile_name_from_account_key(&account),
        launch_mode,
        bound_account: account,
        proxy_url: String::new(), // 旧配置没有自定义地址,启动时回退本机端点。
        model: settings.codex_model.trim().to_string(),
        reasoning,
        api_key: settings.codex_api_key.trim().to_string(),
    });
    // 清空旧平铺字段：迁移后以 codex_profiles 为唯一数据源。
    settings.codex_launch_mode = "app".to_string();
    settings.codex_bound_account = String::new();
    settings.codex_model = String::new();
    settings.codex_reasoning = "high".to_string();
    settings.codex_api_key = String::new();
}

/// 从账号 key（如 `codex-foo@bar.com`）提取一个友好的方案名。
fn profile_name_from_account_key(key: &str) -> String {
    let trimmed = key.trim();
    let friendly = trimmed.strip_prefix("codex-").unwrap_or(trimmed);
    if friendly.is_empty() {
        "默认方案".to_string()
    } else {
        friendly.to_string()
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
    /// 端口监听者的短 TTL 缓存：refresh 被 UI 高频轮询，避免每次都跑 netstat/tasklist。
    port_listener_cache: Option<(Instant, Option<(String, String)>)>,
    /// 当前状态是否是「端口被其它程序占用」：占用解除后用它把状态收回 Stopped。
    port_conflict: bool,
    /// Kiro routes through a kiro-rs sidecar (CLIProxyAPI can't speak
    /// CodeWhisperer); started/stopped together with the core.
    kiro_sidecar: kiro_sidecar::KiroSidecar,
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
            port_listener_cache: None,
            port_conflict: false,
            kiro_sidecar: kiro_sidecar::KiroSidecar::default(),
        }
    }

    /// 带 10 秒 TTL 缓存的 [`port_listener`]。
    fn port_listener_cached(&mut self, port: u16) -> Option<(String, String)> {
        const TTL: Duration = Duration::from_secs(10);
        if let Some((checked_at, listener)) = &self.port_listener_cache {
            if checked_at.elapsed() < TTL {
                return listener.clone();
            }
        }
        let listener = port_listener(port);
        self.port_listener_cache = Some((Instant::now(), listener.clone()));
        listener
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

        // Bring up the Kiro sidecar (no-op unless ≥1 Kiro account); CLIProxyAPI's
        // config.yaml already references it as a claude-api-key provider.
        self.kiro_sidecar.sync_and_start(&self.paths.auth_dir);

        // Pre-flight: if proxy_port is already taken, only reclaim it when the
        // holder is OUR own orphaned proxy. A foreign process is never killed —
        // surface a clear, actionable conflict so the user can change the port.
        if let Some((_pid, holder)) = port_listener(settings.proxy_port) {
            let proxy_bin = managed_binary
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("CLIProxyAPI");
            if is_own_proxy_process_name(&holder, proxy_bin) {
                // Orphaned proxy from a previous session — reclaim the port.
                kill_process_on_port(settings.proxy_port, proxy_bin);
                thread::sleep(Duration::from_millis(400));
            } else {
                let message = format!(
                    "端口 {} 已被『{}』占用，无法启动代理。请在设置中改用其它端口，或关闭占用该端口的程序后重试。",
                    settings.proxy_port, holder
                );
                self.port_conflict = true;
                self.state = state_for_paths(
                    settings,
                    &self.paths,
                    ProxyStatusKind::Crashed,
                    None,
                    None,
                    self.crash_count,
                    ProxyHealthState::unhealthy(now_unix_seconds(), &message),
                    message.clone(),
                );
                return Err(ProxyCoreError::StartupFailed(message));
            }
        }
        // 预检后端口状态已变化（可能刚回收了孤儿代理），作废监听者缓存。
        self.port_listener_cache = None;
        self.port_conflict = false;

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
        // 停止会改变端口状态，作废监听者缓存。
        self.port_listener_cache = None;
        self.port_conflict = false;

        // Tear down the Kiro sidecar alongside the core.
        self.kiro_sidecar.stop();

        if let Some(mut bridge) = self.bridge.take() {
            bridge.stop();
        }

        let Some(mut child) = self.child.take() else {
            // We don't own a child process. If a proxy is nonetheless running
            // (adopted/orphaned, status Running from the health probe), terminate
            // it by binary name so Stop actually stops it instead of flipping
            // back to Running on the next refresh probe.
            if matches!(self.state.status, ProxyStatusKind::Running) {
                // Only terminate the process holding OUR port — never `taskkill
                // /IM` by image name, which would also kill any other CLIProxyAPI
                // the user runs on a different port.
                let proxy_bin = self
                    .paths
                    .managed_binary_path()
                    .file_name()
                    .and_then(|name| name.to_str())
                    .map(str::to_string)
                    .unwrap_or_else(|| "CLIProxyAPI".to_string());
                kill_process_on_port(settings.proxy_port, &proxy_bin);
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

    fn shutdown(&mut self, settings: &AppSettings) {
        self.kiro_sidecar.stop();
        if let Some(mut bridge) = self.bridge.take() {
            bridge.stop();
        }
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        // Also terminate an adopted/external proxy by its listening port, so
        // closing the app doesn't leave the proxy API running in the background.
        let proxy_bin = self
            .paths
            .managed_binary_path()
            .file_name()
            .and_then(|name| name.to_str())
            .map(str::to_string)
            .unwrap_or_else(|| "CLIProxyAPI".to_string());
        kill_process_on_port(settings.proxy_port, &proxy_bin);
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
                // 探测返回 2xx ≠ 一定是我们的代理：对所有路径都回 200 的本地服务
                // （开发服务器等）占了端口也会命中。领养前先确认监听者进程身份，
                // 否则把别人的程序误报成「代理已启动」。
                let proxy_bin = self
                    .paths
                    .managed_binary_path()
                    .file_name()
                    .and_then(|name| name.to_str())
                    .map(str::to_string)
                    .unwrap_or_else(|| "CLIProxyAPI".to_string());
                if let Some((pid, holder)) = self.port_listener_cached(settings.proxy_port) {
                    if !is_own_proxy_process_name(&holder, &proxy_bin) {
                        let message = format!(
                            "端口 {} 已被『{}』(PID {}) 占用，本地代理并未启动。请在设置中改用其它端口，或关闭该程序后重试。",
                            settings.proxy_port, holder, pid
                        );
                        self.port_conflict = true;
                        self.state = state_for_paths(
                            settings,
                            &self.paths,
                            ProxyStatusKind::Crashed,
                            None,
                            None,
                            self.crash_count,
                            ProxyHealthState::unhealthy(now_unix_seconds(), &message),
                            message.clone(),
                        );
                        return;
                    }
                }
                self.port_conflict = false;
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
            } else if self.port_conflict {
                // 占用方已不再响应：解除冲突态，回到「未启动」。
                self.port_conflict = false;
                self.state = state_for_paths(
                    settings,
                    &self.paths,
                    ProxyStatusKind::Stopped,
                    None,
                    None,
                    self.crash_count,
                    ProxyHealthState::unknown("代理核心未运行。"),
                    "端口占用已解除，代理核心尚未启动。",
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

        let kiro_active = kiro_sidecar::kiro_account_count(&self.paths.auth_dir) > 0;

        let mut config = render_proxy_config(
            settings,
            &self.paths,
            &self.management_key,
            &self.local_api_key,
        );
        config.push_str(&custom_providers_yaml());
        // Kiro (CodeWhisperer) isn't a CLIProxyAPI provider; route it through the
        // kiro-rs sidecar, registered here as a claude-api-key provider.
        if kiro_active {
            let bound_keys: Vec<String> = get_api_key_bindings()
                .into_iter()
                .filter(|binding| binding.provider_id == kiro_sidecar::KIRO_PROVIDER_ID)
                .map(|binding| binding.api_key)
                .collect();
            config.push_str(&kiro_sidecar::provider_yaml(
                &kiro_sidecar::current_api_key(),
                &bound_keys,
            ));
        }
        // Per-key→pool gating: when api-keys are bound to pools, load the
        // quotio-key-router scheduler plugin so each key can only reach its pool.
        config.push_str(&key_router_plugins_yaml(&self.paths.proxy_dir));
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
        params.push(format!(
            "        \"reasoning.effort\": {}",
            yaml_quote(reasoning)
        ));
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
/// Best-effort process image name for a PID on Windows (e.g. "CLIProxyAPI.exe").
#[cfg(windows)]
fn process_name_for_pid(pid: &str) -> Option<String> {
    use std::os::windows::process::CommandExt;
    let output = std::process::Command::new("tasklist")
        .args(["/FI", &format!("PID eq {}", pid), "/FO", "CSV", "/NH"])
        .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
    // CSV row: "Image.exe","PID",... → the first quoted field is the image name.
    text.split('"')
        .nth(1)
        .map(str::to_string)
        .filter(|name| !name.is_empty() && !name.contains("INFO"))
}

#[cfg(not(windows))]
fn process_name_for_pid(pid: &str) -> Option<String> {
    #[cfg(target_os = "linux")]
    {
        if let Ok(name) = fs::read_to_string(format!("/proc/{}/comm", pid)) {
            let name = name.trim().to_string();
            if !name.is_empty() {
                return Some(name);
            }
        }
    }

    let output = Command::new("ps")
        .args(["-p", pid, "-o", "comm="])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let name = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}

/// Best-effort (pid, image name) of whatever is LISTENING on `port`, so callers
/// can tell our own orphaned proxy apart from a foreign process.
#[cfg(windows)]
fn port_listener(port: u16) -> Option<(String, String)> {
    use std::os::windows::process::CommandExt;
    let needle = format!(":{}", port);
    let output = std::process::Command::new("netstat")
        .args(["-ano"])
        .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
    let pid = text.lines().find_map(|line| {
        if line.contains("LISTENING") && line.contains(&needle) {
            line.split_whitespace()
                .last()
                .filter(|pid| pid.chars().all(|c| c.is_ascii_digit()) && *pid != "0")
                .map(str::to_string)
        } else {
            None
        }
    })?;
    let name = process_name_for_pid(&pid).unwrap_or_else(|| "未知程序".to_string());
    Some((pid, name))
}

#[cfg(not(windows))]
fn port_listener(port: u16) -> Option<(String, String)> {
    let port_filter = format!("-iTCP:{}", port);
    let output = Command::new("lsof")
        .args(["-nP", &port_filter, "-sTCP:LISTEN", "-t"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let pid = String::from_utf8_lossy(&output.stdout)
        .split_whitespace()
        .find(|pid| pid.chars().all(|c| c.is_ascii_digit()) && *pid != "0")?
        .to_string();
    let name = process_name_for_pid(&pid).unwrap_or_else(|| "未知程序".to_string());
    Some((pid, name))
}

/// 端口监听者的进程名是否是我们自己的代理（CLIProxyAPI）。
/// 用于区分「自家孤儿代理」和「碰巧占了端口的别人程序」。
fn is_own_proxy_process_name(holder: &str, proxy_bin: &str) -> bool {
    let holder = normalized_process_name(holder);
    let proxy_bin = normalized_process_name(proxy_bin);
    !holder.is_empty()
        && ((!proxy_bin.is_empty() && holder == proxy_bin) || holder.contains("cliproxyapi"))
}

fn normalized_process_name(value: &str) -> String {
    value
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// Terminate the proxy listening on `port` — ONLY when it is our own CLIProxyAPI
/// binary, never a foreign process that merely shares the port.
fn kill_process_on_port(port: u16, proxy_bin: &str) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let needle = format!(":{}", port);
        let Ok(output) = std::process::Command::new("netstat")
            .args(["-ano"])
            .creation_flags(0x0800_0000) // CREATE_NO_WINDOW: no console flash
            .output()
        else {
            return;
        };
        let text = String::from_utf8_lossy(&output.stdout);
        let mut pids = std::collections::BTreeSet::new();
        for line in text.lines() {
            if line.contains("LISTENING") && line.contains(&needle) {
                if let Some(pid) = line.split_whitespace().last() {
                    if !pid.is_empty() && pid.chars().all(|c| c.is_ascii_digit()) && pid != "0" {
                        pids.insert(pid.to_string());
                    }
                }
            }
        }
        for pid in pids {
            // Never kill a foreign process that merely shares the port.
            let is_ours = process_name_for_pid(&pid)
                .map(|name| is_own_proxy_process_name(&name, proxy_bin))
                .unwrap_or(false);
            if !is_ours {
                continue;
            }
            let _ = std::process::Command::new("taskkill")
                .args(["/F", "/PID", &pid])
                .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
                .output();
        }
    }
    #[cfg(not(windows))]
    {
        let port_filter = format!("-iTCP:{}", port);
        let Ok(output) = Command::new("lsof")
            .args(["-nP", &port_filter, "-sTCP:LISTEN", "-t"])
            .output()
        else {
            return;
        };
        if !output.status.success() {
            return;
        }
        for pid in String::from_utf8_lossy(&output.stdout)
            .split_whitespace()
            .filter(|pid| pid.chars().all(|c| c.is_ascii_digit()) && *pid != "0")
        {
            let is_ours = process_name_for_pid(pid)
                .map(|name| is_own_proxy_process_name(&name, proxy_bin))
                .unwrap_or(false);
            if !is_ours {
                continue;
            }
            let _ = Command::new("kill").args(["-TERM", pid]).output();
        }
    }
}

/// Probe whether the Codex OAuth callback port (1455) can be bound on loopback.
/// Codex's redirect_uri is always `http://localhost:1455/auth/callback`; if 1455
/// can't be bound, the browser redirect dies with ERR_CONNECTION_REFUSED and the
/// login silently never completes. The classic Windows cause is 1455 falling in a
/// reserved *excluded* port range (Hyper-V / WSL / winnat) where nothing can bind
/// it. Returns Err with actionable guidance in that case; `AddrInUse` is treated
/// as OK (likely CLIProxyAPI's own listener from a prior attempt — a retry is
/// fine), so we only block on the genuinely fatal "can't bind at all" case.
pub fn probe_codex_oauth_port() -> Result<(), String> {
    use std::net::TcpListener;
    const PORT: u16 = 1455;
    match TcpListener::bind(("127.0.0.1", PORT)) {
        Ok(listener) => {
            drop(listener);
            Ok(())
        }
        Err(error) if error.kind() == std::io::ErrorKind::AddrInUse => Ok(()),
        Err(error) => Err(format!(
            "无法绑定 Codex 登录回调端口 1455（{error}）。\n\
             多见于 Windows 把 1455 划进了保留排除端口区间（Hyper-V / WSL / winnat），谁都绑不上——\
             浏览器跳回 http://localhost:1455/auth/callback 时会“无法访问”，登录卡住。\n\
             排查：管理员运行 `netsh int ipv4 show excludedportrange protocol=tcp`，\
             若 1455 落在某区间内，可 `net stop winnat` 再 `net start winnat` 重置，或调整保留区间后重试。"
        )),
    }
}

/// Complete an OAuth login by replaying the browser's callback request to the
/// proxy's *local* callback listener (e.g. `http://localhost:1455/auth/callback
/// ?code=...&state=...`). The browser's own redirect can be swallowed by a
/// system proxy (e.g. Karing) on loopback; issuing the GET from here connects
/// straight to the loopback listener, bypassing any proxy, so CLIProxyAPI
/// receives the `code` and performs the token exchange itself.
pub fn submit_oauth_callback(url: &str) -> Result<(), String> {
    use std::io::{Read as _, Write as _};
    use std::net::{TcpStream, ToSocketAddrs};
    use std::time::Duration;

    let url = url.trim();
    let rest = url
        .strip_prefix("http://")
        .or_else(|| url.strip_prefix("https://"))
        .ok_or_else(|| "回调地址需以 http:// 开头".to_string())?;
    let (authority, path) = match rest.find('/') {
        Some(index) => (&rest[..index], &rest[index..]),
        None => (rest, "/"),
    };
    let (host, port) = authority.rsplit_once(':').unwrap_or((authority, "80"));
    if !matches!(host, "localhost" | "127.0.0.1" | "::1" | "[::1]") {
        return Err(format!(
            "出于安全考虑只允许本地回调地址（localhost/127.0.0.1），收到：{}",
            host
        ));
    }
    let port: u16 = port.parse().map_err(|_| "回调地址端口无效".to_string())?;
    let addrs: Vec<_> = format!("{}:{}", host, port)
        .to_socket_addrs()
        .map_err(|error| format!("解析回调地址失败：{}", error))?
        .collect();
    // Try every resolved address: "localhost" often resolves to IPv6 ::1 first
    // on Windows while the callback listener only binds IPv4 127.0.0.1, so a
    // single ::1 attempt would be refused (os error 10061).
    let mut stream = None;
    let mut last_err = "无可用地址".to_string();
    for addr in &addrs {
        match TcpStream::connect_timeout(addr, Duration::from_secs(5)) {
            Ok(connected) => {
                stream = Some(connected);
                break;
            }
            Err(error) => last_err = format!("{}", error),
        }
    }
    let mut stream =
        stream.ok_or_else(|| format!("连接本地回调失败（{}:{}）：{}", host, port, last_err))?;
    let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
    let request = format!(
        "GET {} HTTP/1.1\r\nHost: {}:{}\r\nConnection: close\r\nAccept: */*\r\n\r\n",
        path, host, port
    );
    stream
        .write_all(request.as_bytes())
        .map_err(|error| format!("发送回调请求失败：{}", error))?;
    let mut response = Vec::new();
    let _ = stream.read_to_end(&mut response); // best-effort; listener may close early
    Ok(())
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

    #[test]
    fn auth_file_ident_keeps_emailless_accounts_distinct() {
        // 两份没 email 的 codex 账号(account_id 在 tokens 里):标识必须不同,否则落到同一个
        // 文件名互相覆盖 —— 多选导入只进来一个的根因。
        let a = serde_json::json!({ "type": "codex", "tokens": { "account_id": "acc-A" } });
        let b = serde_json::json!({ "type": "codex", "tokens": { "account_id": "acc-B" } });
        assert_ne!(auth_file_ident(&a), auth_file_ident(&b));
        assert!(auth_file_ident(&a).contains("acc-A"));

        // 既无 email 也无 account_id 的两份不同内容靠指纹区分;同一份重复导入仍同名(幂等)。
        let c = serde_json::json!({ "type": "codex", "access_token": "tok-c" });
        let d = serde_json::json!({ "type": "codex", "access_token": "tok-d" });
        assert_ne!(auth_file_ident(&c), auth_file_ident(&d));
        assert_eq!(
            auth_file_ident(&c),
            auth_file_ident(&serde_json::json!({ "type": "codex", "access_token": "tok-c" }))
        );

        // 有 email 时优先用 email。
        let e = serde_json::json!({ "type": "codex", "email": "x@e.com", "tokens": { "account_id": "acc-A" } });
        assert_eq!(auth_file_ident(&e), "x@e.com");
    }

    fn quota_with_models(key: &str) -> quotio_types::AccountQuota {
        quotio_types::AccountQuota {
            provider_id: "codex".to_string(),
            account_label: key.to_string(),
            account_key: key.to_string(),
            is_forbidden: false,
            status_message: None,
            models: vec![quotio_types::QuotaModelUsage {
                model: "gpt-5".to_string(),
                used_percent: 20.0,
                remaining_percent: 80.0,
                reset_at: None,
                reset_at_unix: None,
            }],
        }
    }

    fn quota_blank(key: &str) -> quotio_types::AccountQuota {
        quotio_types::AccountQuota {
            provider_id: "codex".to_string(),
            account_label: key.to_string(),
            account_key: key.to_string(),
            is_forbidden: false,
            status_message: None,
            models: Vec::new(),
        }
    }

    #[test]
    fn store_quotas_keeps_last_good_on_transient_blank() {
        let mut core = AppCore::default();
        core.store_quotas(vec![quota_with_models("a"), quota_with_models("b")]);
        // "a" comes back blank (transient probe failure), "b" still good →
        // "a" keeps its previous numbers instead of flapping to "fetch failed".
        core.store_quotas(vec![quota_blank("a"), quota_with_models("b")]);
        let a = core.quotas.iter().find(|item| item.account_key == "a").unwrap();
        assert_eq!(a.models.len(), 1, "transiently-blank 'a' keeps its previous models");
        // An all-blank refresh is a real outage — stored as-is so the UI can flag it.
        core.store_quotas(vec![quota_blank("a"), quota_blank("b")]);
        assert!(
            core.quotas.iter().all(|item| item.models.is_empty()),
            "all-blank refresh stores as-is so the proxy-unreachable banner can fire"
        );
    }
    #[test]
    fn dedup_keeps_team_members_but_collapses_true_dupes() {
        use std::fs;
        let dir = std::env::temp_dir().join(format!("quotio_dedup_team_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let write = |name: &str, email: &str, acct: &str| {
            fs::write(
                dir.join(name),
                format!(r#"{{"type":"codex","email":"{email}","account_id":"{acct}","access_token":"x"}}"#),
            )
            .unwrap();
        };
        // ChatGPT Team: two members share ONE account_id but are separate seats.
        write("a.json", "alice@x.com", "acct-1");
        write("b.json", "bob@x.com", "acct-1");
        // A genuine re-import of alice (same account_id + email) must still collapse.
        write("a2.json", "alice@x.com", "acct-1");
        // A DEAD/stale copy of alice whose account_id can't be parsed (the failing
        // ones that got disabled) must ALSO collapse into alice — these piling up was
        // the "导入 Team 显示重复账号" bug. account_id-in-key never matched this twin.
        fs::write(
            dir.join("a3.json"),
            r#"{"type":"codex","email":"alice@x.com","access_token":"x"}"#,
        )
        .unwrap();

        dedup_codex_auth_keep_newest(&dir, "");

        let remaining: Vec<String> = fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        assert_eq!(
            remaining.len(),
            2,
            "alice (3 copies incl. a dead one → 1) + bob (distinct member): {remaining:?}"
        );
        assert!(remaining.iter().any(|n| n == "b.json"), "distinct Team member kept");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn convert_sub2api_maps_openai_account_to_codex_auth() {
        let input = serde_json::json!({
            "type": "sub2api-data",
            "version": 1,
            "accounts": [{
                "name": "a@x.com", "platform": "openai", "type": "oauth",
                "credentials": {
                    "access_token": "at", "refresh_token": "rt", "id_token": "it",
                    "email": "a@x.com", "chatgpt_account_id": "acc-123",
                    "expires_at": "2026-06-30T07:57:49Z", "client_id": "app_x",
                    "plan_type": "k12"
                }
            }]
        });
        let out = convert_sub2api_accounts(&input).expect("应识别为 sub2api 并转换");
        assert_eq!(out.len(), 1);
        let a = out[0].as_object().unwrap();
        assert_eq!(a.get("type").unwrap(), "codex");
        assert_eq!(a.get("account_id").unwrap(), "acc-123"); // chatgpt_account_id 改名
        assert_eq!(a.get("expired").unwrap(), "2026-06-30T07:57:49Z"); // expires_at 改名
        assert_eq!(a.get("email").unwrap(), "a@x.com");
        assert_eq!(a.get("access_token").unwrap(), "at");
        assert_eq!(a.get("disabled").unwrap(), &serde_json::Value::Bool(false));
        assert!(a.get("client_id").is_none(), "codex auth 不需要 client_id,应丢弃");
        assert!(a.get("plan_type").is_none(), "plan_type 应丢弃");

        // 非 sub2api → None,走原有导入逻辑
        assert!(convert_sub2api_accounts(&serde_json::json!({"type": "codex"})).is_none());
        // 缺 access_token 的账号被跳过 → 整体无可转账号 → None
        let missing = serde_json::json!({
            "type": "sub2api-data",
            "accounts": [{ "platform": "openai", "credentials": { "email": "b@x.com" } }]
        });
        assert!(convert_sub2api_accounts(&missing).is_none());
    }

    #[test]
    fn convert_accounts_export_maps_flat_account_to_codex_auth() {
        // accounts-export.json:扁平驼峰、无 id_token。(占位字符串,不放真实凭证。)
        let input = serde_json::json!({
            "accounts": [{
                "id": "abc", "token": "access-jwt", "refreshToken": "rt-fake",
                "email": "x@example.com", "accountId": "acc-123", "status": "expired"
            }]
        });
        let out = convert_accounts_export(&input).expect("应识别 accounts-export 并转换");
        assert_eq!(out.len(), 1);
        let a = out[0].as_object().unwrap();
        assert_eq!(a.get("type").unwrap(), "codex");
        assert_eq!(a.get("access_token").unwrap(), "access-jwt"); // token → access_token
        assert_eq!(a.get("refresh_token").unwrap(), "rt-fake"); // refreshToken → refresh_token
        assert_eq!(a.get("account_id").unwrap(), "acc-123"); // accountId → account_id
        assert_eq!(a.get("email").unwrap(), "x@example.com");
        assert_eq!(a.get("id_token").unwrap(), ""); // 占位,首次续期补全
        assert_eq!(a.get("disabled").unwrap(), &serde_json::Value::Bool(false));

        // sub2api 交给它自己的转换器
        assert!(convert_accounts_export(&serde_json::json!({"type":"sub2api-data","accounts":[]})).is_none());
        // 缺 refreshToken 的账号被跳过 → 整体 None
        assert!(convert_accounts_export(&serde_json::json!({"accounts":[{"token":"a","email":"x@e.com"}]})).is_none());
        // 普通 JSON 数组不是这个格式 → None(走原有逻辑)
        assert!(convert_accounts_export(&serde_json::json!([{"type":"codex"}])).is_none());
    }

    #[test]
    fn parse_settings_tolerant_salvages_valid_fields_when_one_field_is_incompatible() {
        // 模拟升级:某个枚举字段(theme)用了当前版本不认识的值 → 严格 from_str 失败。
        let json =
            r#"{ "proxy_port": 9999, "language": "zh-CN", "theme": "some_future_theme_variant" }"#;
        assert!(
            serde_json::from_str::<AppSettings>(json).is_err(),
            "坏枚举值应让严格解析失败(否则测不到容错路径)"
        );
        // 容错解析:保留有效字段,只把坏字段回退默认,而不是整份重置。
        let salvaged = parse_settings_tolerant(json).expect("应抢救出设置,而不是返回 None");
        assert_eq!(salvaged.proxy_port, 9999, "有效字段(端口)必须保留");
        assert_eq!(salvaged.language, "zh-CN", "有效字段(语言)必须保留");
        // 完全合法的设置走快路径,原样返回。
        let clean = serde_json::to_string(&AppSettings::default()).unwrap();
        assert!(parse_settings_tolerant(&clean).is_some());
    }

    #[test]
    fn prune_old_error_logs_drops_only_stale_dated_files() {
        use std::fs;
        let dir = std::env::temp_dir().join(format!("quotio_errlog_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        fs::write(dir.join("errors-2000-01-01.jsonl"), "stale").unwrap();
        fs::write(dir.join(format!("errors-{today}.jsonl")), "fresh").unwrap();
        fs::write(dir.join("native-oauth.log"), "unrelated").unwrap();

        prune_old_error_logs(&dir);

        assert!(!dir.join("errors-2000-01-01.jsonl").exists(), "30天前的删掉");
        assert!(dir.join(format!("errors-{today}.jsonl")).exists(), "今天的保留");
        assert!(dir.join("native-oauth.log").exists(), "非 errors-* 文件不动");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn parse_model_list_splits_dedups_and_trims() {
        // Comma, whitespace, and newline separators all work; blanks dropped;
        // first-seen order preserved; duplicates removed.
        let got = parse_model_list("gpt-5.5, claude-sonnet-4-5\n  gpt-5.5\tkimi-k2 ,");
        assert_eq!(got, vec!["gpt-5.5", "claude-sonnet-4-5", "kimi-k2"]);
        // A list of only separators/blanks yields nothing (provider stays unroutable).
        assert!(parse_model_list("   \n , \t ").is_empty());
    }

    use std::{
        net::{TcpListener, TcpStream},
        sync::{Arc, Mutex},
        thread,
        time::{Duration, Instant},
    };

    #[test]
    fn codex_monitor_apply_ignores_stale_generation_and_debounces() {
        let mut core = AppCore::default();
        core.codex_session = Some(codex_launch::CodexSession::new(Some(4242), "app"));
        core.codex_session_generation = 7;

        // 代数不匹配：探测期间发生过停止/重启，丢弃这次结果。
        assert!(!core.codex_monitor_apply(6, false));
        assert!(core.codex_session.is_some());

        // 活着：标记 seen_running 并清零去抖计数。
        assert!(!core.codex_monitor_apply(7, true));
        assert!(core.codex_session.as_ref().unwrap().seen_running);

        // 第一次查不到：去抖，不还原、不清会话。
        assert!(!core.codex_monitor_apply(7, false));
        assert!(core.codex_session.is_some());
        assert_eq!(core.codex_session.as_ref().unwrap().miss_count, 1);
    }

    #[test]
    fn codex_monitor_apply_waits_out_startup_grace_period() {
        let mut core = AppCore::default();
        core.codex_session = Some(codex_launch::CodexSession::new(None, "app"));
        core.codex_session_generation = 1;

        // 刚启动、还没观测到过进程：宽限期内查不到不算退出。
        assert!(!core.codex_monitor_apply(1, false));
        assert!(core.codex_session.is_some());
    }

    #[test]
    fn codex_monitor_probe_targets_match_launch_mode() {
        let mut core = AppCore::default();
        assert!(core.codex_monitor_probe().is_none());

        core.codex_session = Some(codex_launch::CodexSession::new(None, "app"));
        assert!(matches!(
            core.codex_monitor_probe(),
            Some((_, CodexMonitorProbe::AppByName))
        ));

        core.codex_session = Some(codex_launch::CodexSession::new(Some(99), "cli"));
        assert!(matches!(
            core.codex_monitor_probe(),
            Some((_, CodexMonitorProbe::CliByPid(99)))
        ));

        // CLI 模式没拿到终端 pid：无从监控。
        core.codex_session = Some(codex_launch::CodexSession::new(None, "cli"));
        assert!(core.codex_monitor_probe().is_none());
    }

    #[test]
    fn migrate_codex_profiles_seeds_one_profile_from_flat_fields() {
        let mut settings = AppSettings {
            codex_bound_account: "codex-foo@bar.com".to_string(),
            codex_launch_mode: "cli".to_string(),
            codex_model: "gpt-5.5".to_string(),
            codex_reasoning: "xhigh".to_string(),
            codex_api_key: "sk-x".to_string(),
            ..AppSettings::default()
        };
        migrate_codex_profiles(&mut settings);

        assert_eq!(settings.codex_profiles.len(), 1);
        let profile = &settings.codex_profiles[0];
        assert_eq!(profile.name, "foo@bar.com");
        assert_eq!(profile.launch_mode, "cli");
        assert_eq!(profile.bound_account, "codex-foo@bar.com");
        assert_eq!(profile.proxy_url, ""); // 旧配置没有自定义代理地址
        assert_eq!(profile.model, "gpt-5.5");
        assert_eq!(profile.reasoning, "xhigh");
        assert_eq!(profile.api_key, "sk-x");
        // 旧平铺字段已清空（单一数据源）。
        assert!(settings.codex_bound_account.is_empty());
        assert!(settings.codex_model.is_empty());

        // 已有方案时不重复迁移。
        migrate_codex_profiles(&mut settings);
        assert_eq!(settings.codex_profiles.len(), 1);
    }

    #[test]
    fn migrate_codex_profiles_noop_without_bound_account() {
        let mut settings = AppSettings::default();
        migrate_codex_profiles(&mut settings);
        assert!(settings.codex_profiles.is_empty());
    }

    fn usage_event_for_test(source: &str, failed: bool) -> quotio_types::UsageEvent {
        quotio_types::UsageEvent {
            event_hash: "h".to_string(),
            request_id: None,
            timestamp_ms: 0,
            timestamp: String::new(),
            provider: None,
            model: "m".to_string(),
            requested_model: None,
            resolved_model: None,
            endpoint: None,
            method: None,
            path: None,
            auth_type: None,
            auth_index: None,
            source: Some(source.to_string()),
            api_key_hash: None,
            input_tokens: 0,
            output_tokens: 0,
            reasoning_tokens: 0,
            cached_tokens: 0,
            cache_creation_tokens: 0,
            cache_read_tokens: 0,
            total_tokens: 0,
            latency_ms: 0,
            failed,
            status_code: Some(if failed { 429 } else { 200 }),
            reasoning_effort: None,
            raw_json: None,
        }
    }

    #[test]
    fn scheduler_rechecks_on_target_failures_with_cooldown() {
        let mut core = AppCore::default();
        core.settings.scheduler_rule = "reset_soonest".to_string();
        core.schedulers.insert(
            "codex".to_string(),
            ProviderSchedulerState {
                current: Some(("codex-a.json".to_string(), Instant::now())),
                target_label: Some("a@example.com".to_string()),
                current_reset_at: None,
                standby_count: 0,
                failure_recheck_at: None,
                exhausted: Default::default(),
            },
        );

        // 别的账号失败 / 目标成功:不触发。
        assert!(!core.scheduler_should_recheck_for_failures(&[
            usage_event_for_test("b@example.com", true),
            usage_event_for_test("a@example.com", false),
        ]));
        // 目标失败:触发一次。
        assert!(core
            .scheduler_should_recheck_for_failures(&[usage_event_for_test("a@example.com", true)]));
        // 冷却期内再失败:不重复触发。
        assert!(!core
            .scheduler_should_recheck_for_failures(&[usage_event_for_test("a@example.com", true)]));

        // 规则关闭:永不触发。
        core.settings.scheduler_rule = "off".to_string();
        core.schedulers.get_mut("codex").unwrap().failure_recheck_at = None;
        assert!(!core
            .scheduler_should_recheck_for_failures(&[usage_event_for_test("a@example.com", true)]));
    }

    #[test]
    fn own_proxy_listener_is_recognized_and_foreign_is_not() {
        assert!(is_own_proxy_process_name("CLIProxyAPI.exe", "CLIProxyAPI.exe"));
        assert!(is_own_proxy_process_name("cliproxyapi", "CLIProxyAPI.exe"));
        assert!(is_own_proxy_process_name(
            "cli-proxy-api",
            "CLIProxyAPI.exe"
        ));
        // 自定义二进制名：与托管二进制同名也算自己的。
        assert!(is_own_proxy_process_name("MyProxy.exe", "myproxy.exe"));
        // 别人的程序占了端口，不能当成已启动的代理。
        assert!(!is_own_proxy_process_name("node.exe", "CLIProxyAPI.exe"));
        assert!(!is_own_proxy_process_name("未知程序", "CLIProxyAPI.exe"));
    }

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
        let listener = TcpListener::bind("127.0.0.1:0").expect("unused port should bind");
        let unused_port = listener
            .local_addr()
            .expect("unused port should have local address")
            .port();
        drop(listener);

        let mut core = AppCore::default();
        core.settings.connection_mode = ConnectionMode::Local;
        core.settings.proxy_port = unused_port;
        core.settings.remote_endpoint_url = None;
        core.settings.remote_management_key = None;
        core.proxy.sync_settings(&core.settings);

        let error = core
            .management_client()
            .expect_err("local management should require a running proxy");

        assert!(error.to_string().contains("代理核心未运行"));
    }

    /// Redirects [`api_keys_path`] to a throwaway temp file seeded with `keys`, so
    /// management-snapshot tests assert against the fixture instead of the developer's
    /// real on-disk `api-keys.json`. Restores the override and deletes the temp dir on
    /// drop (even on panic). The override is process-global, so only one fixture may be
    /// live at a time — fine while a single test uses it.
    struct ApiKeysFixture {
        dir: PathBuf,
    }

    impl ApiKeysFixture {
        fn new(keys: &[&str]) -> Self {
            let dir = std::env::temp_dir().join(format!("quotio-api-keys-test-{}", Uuid::new_v4()));
            fs::create_dir_all(&dir).expect("fixture temp dir should be creatable");
            let body = serde_json::to_string(keys).expect("fixture keys should serialize");
            fs::write(dir.join("api-keys.json"), body).expect("fixture api-keys.json should write");
            set_api_keys_path_override(Some(dir.join("api-keys.json")));
            Self { dir }
        }
    }

    impl Drop for ApiKeysFixture {
        fn drop(&mut self) {
            set_api_keys_path_override(None);
            let _ = fs::remove_dir_all(&self.dir);
        }
    }

    #[tokio::test]
    async fn app_core_refreshes_management_snapshot_from_configured_endpoint() {
        // Isolate from the real on-disk api-keys file so `state.api_keys` reflects this
        // fixture, not whatever proxy key the developer has configured locally.
        let _api_keys = ApiKeysFixture::new(&["sk-local-secret"]);
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
