use std::sync::{Arc, Mutex};

use quotio_core::{management::ManagementApiClient, AppCore};
use quotio_types::{
    AccountAuthHealth, AccountSummaryRow, AgentBackupFile, AgentConfigurationRequest,
    AgentConfigurationResult, AppSettings, AppState, AuthFile, AvailableModel, CredentialStatus,
    FallbackConfigAction, ManagementSnapshot, ModelPrice, OAuthStatusResponse, OAuthUrlResponse,
    PlatformInfo, SavedAgentConfiguration, UsageAggregate, UsageChartBucket, UsageFilterOptions,
    UsageModelBreakdownRow, UsageQuery, UsageTimeSeriesPoint,
};
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager, State,
};
use tauri_plugin_autostart::ManagerExt;

struct DesktopState {
    core: Arc<Mutex<AppCore>>,
    tunnel: Mutex<TunnelRuntime>,
}

#[derive(Default)]
struct TunnelRuntime {
    child: Option<std::process::Child>,
    public_url: Option<String>,
}

#[derive(serde::Serialize)]
struct TunnelStatus {
    running: bool,
    public_url: Option<String>,
    has_binary: bool,
}

#[tauri::command]
fn get_app_state(state: State<'_, DesktopState>) -> Result<AppState, String> {
    let mut core = state
        .core
        .lock()
        .map_err(|_| "无法读取应用状态".to_string())?;
    Ok(core.app_state())
}

#[tauri::command]
fn get_platform_info(state: State<'_, DesktopState>) -> Result<PlatformInfo, String> {
    let core = state
        .core
        .lock()
        .map_err(|_| "无法读取平台信息".to_string())?;
    Ok(core.platform_info())
}

#[tauri::command]
fn save_settings(
    settings: AppSettings,
    state: State<'_, DesktopState>,
) -> Result<AppState, String> {
    let mut core = state.core.lock().map_err(|_| "无法保存设置".to_string())?;
    core.save_settings(settings)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn update_fallback_configuration(
    action: FallbackConfigAction,
    state: State<'_, DesktopState>,
) -> Result<AppState, String> {
    let mut core = state
        .core
        .lock()
        .map_err(|_| "无法更新 fallback 配置".to_string())?;
    core.update_fallback_configuration(action)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn detect_agents(state: State<'_, DesktopState>) -> Result<AppState, String> {
    let mut core = state
        .core
        .lock()
        .map_err(|_| "无法检测 CLI agents".to_string())?;
    Ok(core.detect_agents())
}

#[tauri::command]
fn read_agent_configuration(
    agent_id: String,
    state: State<'_, DesktopState>,
) -> Result<SavedAgentConfiguration, String> {
    let core = state
        .core
        .lock()
        .map_err(|_| "无法读取 agent 配置".to_string())?;
    core.read_agent_configuration(&agent_id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn configure_agent(
    request: AgentConfigurationRequest,
    state: State<'_, DesktopState>,
) -> Result<AgentConfigurationResult, String> {
    let mut core = state
        .core
        .lock()
        .map_err(|_| "无法配置 agent".to_string())?;
    core.configure_agent_with_result(request)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn detect_codex_app() -> Option<String> {
    quotio_core::codex_launch::detect_codex_app_path()
        .map(|path| path.to_string_lossy().to_string())
}

#[tauri::command]
fn list_codex_launch_accounts() -> Vec<quotio_core::codex_launch::CodexAccountRef> {
    quotio_core::codex_launch::list_codex_accounts()
}

/// 一键启动 Codex（用已保存的 codex_* 设置）：确保代理 → 备份原始 → 写配置 → 注入账号 → 启动。
#[tauri::command]
fn codex_start(state: State<'_, DesktopState>) -> Result<String, String> {
    let mut core = state
        .core
        .lock()
        .map_err(|_| "无法启动 Codex".to_string())?;
    core.codex_start().map_err(|error| error.to_string())
}

/// 停止 Codex：杀掉启动的进程 + 把 ~/.codex 还原到启动前。
#[tauri::command]
fn codex_stop(state: State<'_, DesktopState>) -> Result<String, String> {
    let mut core = state
        .core
        .lock()
        .map_err(|_| "无法停止 Codex".to_string())?;
    core.codex_stop().map_err(|error| error.to_string())
}

/// 当前 Codex 一键启动会话是否在运行。
#[tauri::command]
fn codex_launch_active(state: State<'_, DesktopState>) -> Result<bool, String> {
    let core = state
        .core
        .lock()
        .map_err(|_| "无法读取 Codex 状态".to_string())?;
    Ok(core.codex_active())
}

/// 拉取代理实际服务的 codex 模型（前端模型下拉用）。best-effort：拿不到返回空，前端回退内置列表。
#[tauri::command]
fn fetch_codex_models(state: State<'_, DesktopState>) -> Result<Vec<String>, String> {
    let (endpoint, api_key) = {
        let core = state
            .core
            .lock()
            .map_err(|_| "无法读取 Codex 模型".to_string())?;
        core.codex_model_fetch_params()
    };
    Ok(quotio_core::codex_launch::fetch_proxy_codex_models(
        &endpoint, &api_key,
    ))
}

#[tauri::command]
fn list_agent_backups(
    agent_id: String,
    state: State<'_, DesktopState>,
) -> Result<Vec<AgentBackupFile>, String> {
    let core = state
        .core
        .lock()
        .map_err(|_| "无法读取 agent 备份".to_string())?;
    core.list_agent_backups(&agent_id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn restore_agent_backup(
    agent_id: String,
    backup_path: String,
    state: State<'_, DesktopState>,
) -> Result<AgentConfigurationResult, String> {
    let mut core = state
        .core
        .lock()
        .map_err(|_| "无法恢复 agent 备份".to_string())?;
    core.restore_agent_backup(&agent_id, &backup_path)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn reset_agent_configuration(
    agent_id: String,
    state: State<'_, DesktopState>,
) -> Result<AgentConfigurationResult, String> {
    let mut core = state
        .core
        .lock()
        .map_err(|_| "无法重置 agent 配置".to_string())?;
    core.reset_agent_configuration(&agent_id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn discover_available_models(
    state: State<'_, DesktopState>,
) -> Result<Vec<AvailableModel>, String> {
    let mut core = state.core.lock().map_err(|_| "无法发现模型".to_string())?;
    Ok(core
        .discover_available_models()
        .fallback_runtime
        .available_models)
}

#[tauri::command]
fn refresh_fallback_route_state(state: State<'_, DesktopState>) -> Result<AppState, String> {
    let mut core = state
        .core
        .lock()
        .map_err(|_| "无法刷新 fallback route state".to_string())?;
    Ok(core.refresh_fallback_route_state())
}

#[tauri::command]
fn credential_status(state: State<'_, DesktopState>) -> Result<CredentialStatus, String> {
    let core = state
        .core
        .lock()
        .map_err(|_| "无法读取凭据状态".to_string())?;
    Ok(core.credential_status())
}

#[tauri::command]
fn clear_remote_management_key(state: State<'_, DesktopState>) -> Result<AppState, String> {
    let mut core = state
        .core
        .lock()
        .map_err(|_| "无法清理远程管理密钥".to_string())?;
    core.clear_remote_management_key()
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn open_config_root(state: State<'_, DesktopState>) -> Result<(), String> {
    let core = state
        .core
        .lock()
        .map_err(|_| "无法打开配置目录".to_string())?;
    core.open_config_root().map_err(|error| error.to_string())
}

#[tauri::command]
fn set_launch_at_login(
    enabled: bool,
    app: AppHandle,
    state: State<'_, DesktopState>,
) -> Result<AppState, String> {
    if enabled {
        app.autolaunch()
            .enable()
            .map_err(|error| error.to_string())?;
    } else {
        app.autolaunch()
            .disable()
            .map_err(|error| error.to_string())?;
    }

    let mut core = state
        .core
        .lock()
        .map_err(|_| "无法保存开机自启状态".to_string())?;
    let mut settings = core.app_state().settings;
    settings.launch_at_login = app.autolaunch().is_enabled().unwrap_or(enabled);
    core.save_settings(settings)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn request_notification_permission() -> Result<bool, String> {
    Ok(true)
}

#[tauri::command]
fn send_test_notification() -> Result<(), String> {
    Ok(())
}

#[tauri::command]
async fn start_proxy(app: AppHandle, state: State<'_, DesktopState>) -> Result<AppState, String> {
    // The proxy start spawns the process + waits ~900ms to detect an immediate
    // crash + probes health, so run it on a blocking thread to never freeze UI.
    let core = Arc::clone(&state.core);
    let outcome = tauri::async_runtime::spawn_blocking(move || {
        let mut core = core.lock().map_err(|_| "无法启动代理核心".to_string())?;
        core.start_proxy().map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| format!("启动任务异常：{}", error))?;
    // Notify every window (main + menu bar) so proxy status stays in sync.
    let _ = app.emit("proxy-changed", ());
    outcome
}

#[tauri::command]
async fn stop_proxy(app: AppHandle, state: State<'_, DesktopState>) -> Result<AppState, String> {
    let core = Arc::clone(&state.core);
    let outcome = tauri::async_runtime::spawn_blocking(move || {
        let mut core = core.lock().map_err(|_| "无法停止代理核心".to_string())?;
        core.stop_proxy().map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| format!("停止任务异常：{}", error))?;
    let _ = app.emit("proxy-changed", ());
    outcome
}

#[tauri::command]
fn restart_proxy(state: State<'_, DesktopState>) -> Result<AppState, String> {
    let mut core = state
        .core
        .lock()
        .map_err(|_| "无法重启代理核心".to_string())?;
    core.restart_proxy().map_err(|error| error.to_string())
}

#[tauri::command]
async fn download_proxy_binary(
    app: AppHandle,
    state: State<'_, DesktopState>,
) -> Result<AppState, String> {
    // Resolve the destination path under a short-lived lock, then release it so
    // the (slow, blocking) network download never holds the core mutex or the
    // main thread — keeping the UI fully responsive.
    let dest = {
        let core = state
            .core
            .lock()
            .map_err(|_| "无法访问代理核心".to_string())?;
        core.proxy_managed_binary_path()
    };

    let progress_app = app.clone();
    let tag = tauri::async_runtime::spawn_blocking(move || {
        let mut last_percent = u8::MAX;
        quotio_core::proxy_download::download_proxy_binary(&dest, |downloaded, total| {
            if total == 0 {
                return;
            }
            let percent = (downloaded.saturating_mul(100) / total).min(100) as u8;
            if percent != last_percent {
                last_percent = percent;
                let _ = progress_app.emit("proxy-download-progress", percent);
            }
        })
    })
    .await
    .map_err(|error| format!("下载任务异常：{}", error))??;

    let _ = app.emit("proxy-download-progress", 100u8);
    let mut core = state
        .core
        .lock()
        .map_err(|_| "无法访问代理核心".to_string())?;
    Ok(core.finalize_proxy_download(tag))
}

fn current_tunnel_status(state: &State<'_, DesktopState>) -> TunnelStatus {
    let has_binary = state
        .core
        .lock()
        .map(|core| core.cloudflared_binary_path().exists())
        .unwrap_or(false);
    let (running, public_url) = state
        .tunnel
        .lock()
        .map(|guard| (guard.child.is_some(), guard.public_url.clone()))
        .unwrap_or((false, None));
    TunnelStatus {
        running,
        public_url,
        has_binary,
    }
}

#[tauri::command]
fn tunnel_status(state: State<'_, DesktopState>) -> Result<TunnelStatus, String> {
    Ok(current_tunnel_status(&state))
}

#[tauri::command]
async fn download_cloudflared(
    app: AppHandle,
    state: State<'_, DesktopState>,
) -> Result<TunnelStatus, String> {
    let dest = {
        let core = state
            .core
            .lock()
            .map_err(|_| "无法访问代理核心".to_string())?;
        core.cloudflared_binary_path()
    };
    let progress_app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut last_percent = u8::MAX;
        quotio_core::tunnel::download_cloudflared(&dest, |downloaded, total| {
            if total == 0 {
                return;
            }
            let percent = (downloaded.saturating_mul(100) / total).min(100) as u8;
            if percent != last_percent {
                last_percent = percent;
                let _ = progress_app.emit("cloudflared-download-progress", percent);
            }
        })
    })
    .await
    .map_err(|error| format!("下载任务异常：{}", error))??;
    let _ = app.emit("cloudflared-download-progress", 100u8);
    Ok(current_tunnel_status(&state))
}

#[tauri::command]
fn start_tunnel(app: AppHandle, state: State<'_, DesktopState>) -> Result<TunnelStatus, String> {
    let (binary, port) = {
        let core = state
            .core
            .lock()
            .map_err(|_| "无法访问代理核心".to_string())?;
        (core.cloudflared_binary_path(), core.proxy_port())
    };
    if !binary.exists() {
        return Err("cloudflared 尚未下载。".to_string());
    }

    {
        let mut guard = state
            .tunnel
            .lock()
            .map_err(|_| "无法访问隧道状态".to_string())?;
        if guard.child.is_some() {
            return Err("隧道已在运行。".to_string());
        }

        let mut command = std::process::Command::new(&binary);
        command
            .args(["tunnel", "--url", &format!("http://localhost:{}", port)])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }

        let mut child = command
            .spawn()
            .map_err(|error| format!("启动隧道失败：{}", error))?;
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        guard.child = Some(child);
        guard.public_url = None;

        if let Some(stdout) = stdout {
            spawn_tunnel_url_reader(stdout, app.clone());
        }
        if let Some(stderr) = stderr {
            spawn_tunnel_url_reader(stderr, app.clone());
        }
    }

    Ok(current_tunnel_status(&state))
}

#[tauri::command]
fn stop_tunnel(state: State<'_, DesktopState>) -> Result<TunnelStatus, String> {
    {
        let mut guard = state
            .tunnel
            .lock()
            .map_err(|_| "无法访问隧道状态".to_string())?;
        if let Some(mut child) = guard.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        guard.public_url = None;
    }
    Ok(current_tunnel_status(&state))
}

fn spawn_tunnel_url_reader<R: std::io::Read + Send + 'static>(reader: R, app: AppHandle) {
    std::thread::spawn(move || {
        use std::io::BufRead;
        let buffered = std::io::BufReader::new(reader);
        for line in buffered.lines() {
            let Ok(line) = line else { break };
            if let Some(url) = quotio_core::tunnel::extract_tunnel_url(&line) {
                if let Some(state) = app.try_state::<DesktopState>() {
                    if let Ok(mut guard) = state.tunnel.lock() {
                        guard.public_url = Some(url.clone());
                    }
                }
                let _ = app.emit("tunnel-url", url);
                break;
            }
        }
    });
}

#[tauri::command]
async fn warmup_accounts(state: State<'_, DesktopState>) -> Result<u32, String> {
    let client = management_client(&state, "无法预热账号")?;
    let auth_files = client
        .fetch_auth_files()
        .await
        .map_err(|error| error.to_string())?;
    let mut warmed = 0u32;
    for file in auth_files {
        let is_antigravity = file.provider.to_lowercase().contains("antigravity")
            || file.name.to_lowercase().contains("antigravity");
        if !is_antigravity || file.disabled {
            continue;
        }
        let Some(auth_index) = file.auth_index.clone() else {
            continue;
        };
        if client
            .warmup_antigravity(&auth_index, "gemini-3-pro-preview")
            .await
            .map(|status| (200..300).contains(&status))
            .unwrap_or(false)
        {
            warmed += 1;
        }
    }
    Ok(warmed)
}

#[tauri::command]
fn show_main_window(app: AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
    if let Some(panel) = app.get_webview_window("menubar") {
        let _ = panel.hide();
    }
    Ok(())
}

#[tauri::command]
fn quit_app(app: AppHandle) {
    app.exit(0);
}

#[tauri::command]
fn show_menubar(app: AppHandle) {
    if let Some(panel) = app.get_webview_window("menubar") {
        position_menubar(&panel);
    }
}

#[tauri::command]
fn get_autostart(app: AppHandle) -> Result<bool, String> {
    app.autolaunch()
        .is_enabled()
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn set_autostart(app: AppHandle, enabled: bool) -> Result<bool, String> {
    let manager = app.autolaunch();
    if enabled {
        manager.enable().map_err(|error| error.to_string())?;
    } else {
        manager.disable().map_err(|error| error.to_string())?;
    }
    manager.is_enabled().map_err(|error| error.to_string())
}

#[tauri::command]
fn check_proxy_health(state: State<'_, DesktopState>) -> Result<AppState, String> {
    let mut core = state
        .core
        .lock()
        .map_err(|_| "无法检查代理健康状态".to_string())?;
    Ok(core.check_proxy_health())
}

#[tauri::command]
async fn refresh_management_state(state: State<'_, DesktopState>) -> Result<AppState, String> {
    let client = management_client(&state, "无法刷新管理接口状态")?;
    // Keep usage telemetry on so the queue fills; draining it is the background
    // collector's job (the single consumer that persists to the usage store).
    let _ = client.set_usage_statistics_enabled(true).await;
    let snapshot = client
        .refresh_snapshot()
        .await
        .map_err(|error| error.to_string())?;
    apply_management_snapshot(&state, snapshot, "无法回写管理接口状态")
}

#[tauri::command]
async fn drain_request_logs(state: State<'_, DesktopState>) -> Result<AppState, String> {
    // Manual queue drain: pull the proxy usage queue (destructive, 60s retention)
    // and persist events to the usage store. Safe alongside the background
    // collector — both write to the same deduplicated store. No-op when
    // management isn't reachable.
    let Ok(client) = management_client(&state, "") else {
        let mut core = state.core.lock().map_err(|_| "无法读取状态".to_string())?;
        return Ok(core.app_state());
    };
    let _ = client.set_usage_statistics_enabled(true).await;
    let events = client.fetch_usage_events(2000).await.unwrap_or_default();
    let store = {
        let core = state.core.lock().map_err(|_| "无法读取状态".to_string())?;
        core.usage_store()
    };
    if !events.is_empty() {
        store.insert_events(&events);
    }
    let mut core = state.core.lock().map_err(|_| "无法读取状态".to_string())?;
    Ok(core.app_state())
}

#[tauri::command]
fn query_usage_stats(
    query: UsageQuery,
    state: State<'_, DesktopState>,
) -> Result<UsageAggregate, String> {
    let core = state
        .core
        .lock()
        .map_err(|_| "无法读取用量统计".to_string())?;
    Ok(core.query_usage_stats(&query))
}

#[tauri::command]
fn query_account_summary(
    query: UsageQuery,
    state: State<'_, DesktopState>,
) -> Result<Vec<AccountSummaryRow>, String> {
    let core = state
        .core
        .lock()
        .map_err(|_| "无法读取账号汇总".to_string())?;
    Ok(core.query_account_summary(&query))
}

#[tauri::command]
fn query_usage_timeseries(
    query: UsageQuery,
    bucket: UsageChartBucket,
    state: State<'_, DesktopState>,
) -> Result<Vec<UsageTimeSeriesPoint>, String> {
    let core = state
        .core
        .lock()
        .map_err(|_| "无法读取用量趋势".to_string())?;
    Ok(core.query_usage_timeseries(&query, bucket))
}

#[tauri::command]
fn query_usage_model_breakdown(
    query: UsageQuery,
    limit: Option<usize>,
    state: State<'_, DesktopState>,
) -> Result<Vec<UsageModelBreakdownRow>, String> {
    let core = state
        .core
        .lock()
        .map_err(|_| "无法读取模型排行".to_string())?;
    Ok(core.query_usage_model_breakdown(&query, limit.unwrap_or(10)))
}

#[tauri::command]
fn list_usage_filter_options(state: State<'_, DesktopState>) -> Result<UsageFilterOptions, String> {
    let core = state
        .core
        .lock()
        .map_err(|_| "无法读取筛选项".to_string())?;
    Ok(core.usage_filter_options())
}

#[tauri::command]
fn query_account_auth_health(
    state: State<'_, DesktopState>,
) -> Result<Vec<AccountAuthHealth>, String> {
    let core = state
        .core
        .lock()
        .map_err(|_| "无法读取账号健康".to_string())?;
    Ok(core.account_auth_health())
}

#[tauri::command]
fn get_model_prices(state: State<'_, DesktopState>) -> Result<Vec<ModelPrice>, String> {
    let core = state
        .core
        .lock()
        .map_err(|_| "无法读取模型单价".to_string())?;
    Ok(core.model_prices())
}

#[tauri::command]
fn set_model_prices(
    prices: Vec<ModelPrice>,
    state: State<'_, DesktopState>,
) -> Result<Vec<ModelPrice>, String> {
    let core = state
        .core
        .lock()
        .map_err(|_| "无法保存模型单价".to_string())?;
    core.set_model_prices(&prices);
    Ok(core.model_prices())
}

fn provider_short(id: &str) -> &str {
    match id {
        "codex" => "Codex",
        "claude" => "Claude",
        "copilot" => "Copilot",
        "antigravity" => "Antigravity",
        "kiro" => "Kiro",
        "glm" => "GLM",
        "trae" => "Trae",
        other => other,
    }
}

#[tauri::command]
fn import_auth_file(
    filename: String,
    content: String,
    state: State<'_, DesktopState>,
) -> Result<AppState, String> {
    let mut core = state.core.lock().map_err(|_| "无法导入账号".to_string())?;
    core.import_auth_file(&filename, &content)
}

#[tauri::command]
fn list_local_accounts() -> Vec<AuthFile> {
    quotio_core::list_local_accounts()
}

#[tauri::command]
fn list_custom_providers() -> Vec<quotio_core::CustomProvider> {
    quotio_core::list_custom_providers()
}

#[tauri::command]
fn add_custom_provider(
    name: String,
    base_url: String,
    api_key: String,
    kind: String,
    prefix: String,
) -> Result<Vec<quotio_core::CustomProvider>, String> {
    quotio_core::add_custom_provider(name, base_url, api_key, kind, prefix)
}

#[tauri::command]
fn delete_custom_provider(id: String) -> Result<Vec<quotio_core::CustomProvider>, String> {
    quotio_core::delete_custom_provider(&id)
}

#[tauri::command]
fn update_custom_provider(
    id: String,
    name: String,
    base_url: String,
    api_key: String,
    kind: String,
    prefix: String,
) -> Result<Vec<quotio_core::CustomProvider>, String> {
    quotio_core::update_custom_provider(id, name, base_url, api_key, kind, prefix)
}

#[tauri::command]
fn list_api_keys() -> Vec<String> {
    quotio_core::get_api_keys()
}

#[tauri::command]
fn add_api_key(key: String, state: State<'_, DesktopState>) -> Result<AppState, String> {
    quotio_core::add_api_key(key)?;
    let mut core = state.core.lock().map_err(|_| "无法访问核心".to_string())?;
    Ok(core.app_state())
}

#[tauri::command]
fn remove_api_key(key: String, state: State<'_, DesktopState>) -> Result<AppState, String> {
    quotio_core::remove_api_key(&key)?;
    let mut core = state.core.lock().map_err(|_| "无法访问核心".to_string())?;
    Ok(core.app_state())
}

#[tauri::command]
fn update_api_key(
    key: String,
    replacement: String,
    state: State<'_, DesktopState>,
) -> Result<AppState, String> {
    quotio_core::update_api_key(&key, replacement)?;
    let mut core = state.core.lock().map_err(|_| "无法访问核心".to_string())?;
    Ok(core.app_state())
}

#[tauri::command]
async fn refresh_quotas(
    app: AppHandle,
    state: State<'_, DesktopState>,
) -> Result<AppState, String> {
    // Resolve the user-configured upstream proxy under a short lock, then release
    // it so the (blocking, multi-provider) network fetch never holds the core
    // mutex or freezes the UI. Provider requests route through that proxy (like
    // the macOS reference app), so accounts no longer vanish when the endpoints
    // are unreachable directly.
    let proxy_url = {
        let core = state
            .core
            .lock()
            .map_err(|_| "无法访问代理核心".to_string())?;
        core.proxy_upstream_url()
    };
    // Stream each account to the UI (event "quota-account") the moment it is
    // fetched, so accounts appear one-by-one and one unreachable account never
    // blocks the rest. The full list is still returned at the end as a sync.
    let app_emit = app.clone();
    let quotas = tauri::async_runtime::spawn_blocking(move || {
        quotio_core::quota::fetch_all_quotas_streaming(proxy_url.as_deref(), &move |account| {
            let _ = app_emit.emit("quota-account", account);
        })
    })
    .await
    .map_err(|error| format!("额度刷新任务异常：{}", error))?;

    // "Quota at a glance" tray tooltip: lowest remaining % per provider.
    let mut by_provider: std::collections::BTreeMap<String, f64> =
        std::collections::BTreeMap::new();
    for account in &quotas {
        for model in &account.models {
            let entry = by_provider
                .entry(account.provider_id.clone())
                .or_insert(100.0);
            *entry = entry.min(model.remaining_percent);
        }
    }
    let tooltip = if by_provider.is_empty() {
        "Quotio".to_string()
    } else {
        by_provider
            .iter()
            .map(|(provider, remaining)| {
                format!("{} {}%", provider_short(provider), remaining.round() as i64)
            })
            .collect::<Vec<_>>()
            .join(" · ")
    };
    if let Some(tray) = app.tray_by_id("main-tray") {
        let _ = tray.set_tooltip(Some(tooltip.as_str()));
    }

    let mut core = state.core.lock().map_err(|_| "无法刷新额度".to_string())?;
    Ok(core.set_quotas(quotas))
}

#[tauri::command]
async fn get_management_debug(state: State<'_, DesktopState>) -> Result<bool, String> {
    let client = management_client(&state, "无法读取 debug 状态")?;
    client.get_debug().await.map_err(|error| error.to_string())
}

#[tauri::command]
async fn set_management_debug(
    enabled: bool,
    state: State<'_, DesktopState>,
) -> Result<AppState, String> {
    let client = management_client(&state, "无法写入 debug 状态")?;
    client
        .set_debug(enabled)
        .await
        .map_err(|error| error.to_string())?;
    refresh_snapshot_with_client(&state, client, "无法刷新 debug 写入后的状态").await
}

#[tauri::command]
async fn get_management_routing_strategy(state: State<'_, DesktopState>) -> Result<String, String> {
    let client = management_client(&state, "无法读取路由策略")?;
    client
        .get_routing_strategy()
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn set_management_routing_strategy(
    strategy: String,
    state: State<'_, DesktopState>,
) -> Result<AppState, String> {
    let client = management_client(&state, "无法写入路由策略")?;
    client
        .set_routing_strategy(strategy)
        .await
        .map_err(|error| error.to_string())?;
    refresh_snapshot_with_client(&state, client, "无法刷新路由写入后的状态").await
}

#[tauri::command]
async fn get_management_proxy_url(state: State<'_, DesktopState>) -> Result<String, String> {
    let client = management_client(&state, "无法读取代理 URL")?;
    client
        .get_proxy_url()
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn set_management_proxy_url(
    url: String,
    state: State<'_, DesktopState>,
) -> Result<AppState, String> {
    let client = management_client(&state, "无法写入代理 URL")?;
    client
        .set_proxy_url(url)
        .await
        .map_err(|error| error.to_string())?;
    refresh_snapshot_with_client(&state, client, "无法刷新代理 URL 写入后的状态").await
}

#[tauri::command]
async fn clear_management_proxy_url(state: State<'_, DesktopState>) -> Result<AppState, String> {
    let client = management_client(&state, "无法清空代理 URL")?;
    client
        .delete_proxy_url()
        .await
        .map_err(|error| error.to_string())?;
    refresh_snapshot_with_client(&state, client, "无法刷新代理 URL 清空后的状态").await
}

#[tauri::command]
async fn set_management_request_log(
    enabled: bool,
    state: State<'_, DesktopState>,
) -> Result<AppState, String> {
    let client = management_client(&state, "无法写入请求日志开关")?;
    client
        .set_request_log(enabled)
        .await
        .map_err(|error| error.to_string())?;
    refresh_snapshot_with_client(&state, client, "无法刷新请求日志写入后的状态").await
}

#[tauri::command]
async fn set_management_request_retry(
    count: u16,
    state: State<'_, DesktopState>,
) -> Result<AppState, String> {
    let client = management_client(&state, "无法写入请求重试次数")?;
    client
        .set_request_retry(count)
        .await
        .map_err(|error| error.to_string())?;
    refresh_snapshot_with_client(&state, client, "无法刷新重试写入后的状态").await
}

#[tauri::command]
async fn clear_management_logs(state: State<'_, DesktopState>) -> Result<AppState, String> {
    let client = management_client(&state, "无法清空管理日志")?;
    client
        .clear_logs()
        .await
        .map_err(|error| error.to_string())?;
    refresh_snapshot_with_client(&state, client, "无法刷新日志清空后的状态").await
}

#[tauri::command]
async fn add_management_api_key(
    key: String,
    state: State<'_, DesktopState>,
) -> Result<AppState, String> {
    let client = management_client(&state, "无法新增 API key")?;
    client
        .add_api_key(key)
        .await
        .map_err(|error| error.to_string())?;
    refresh_snapshot_with_client(&state, client, "无法刷新 API key 新增后的状态").await
}

#[tauri::command]
async fn update_management_api_key(
    key: String,
    replacement: String,
    state: State<'_, DesktopState>,
) -> Result<AppState, String> {
    let client = management_client(&state, "无法更新 API key")?;
    client
        .update_api_key(key, replacement)
        .await
        .map_err(|error| error.to_string())?;
    refresh_snapshot_with_client(&state, client, "无法刷新 API key 更新后的状态").await
}

#[tauri::command]
async fn delete_management_api_key(
    value: String,
    state: State<'_, DesktopState>,
) -> Result<AppState, String> {
    let client = management_client(&state, "无法删除 API key")?;
    client
        .delete_api_key(&value)
        .await
        .map_err(|error| error.to_string())?;
    refresh_snapshot_with_client(&state, client, "无法刷新 API key 删除后的状态").await
}

#[tauri::command]
async fn delete_management_api_key_by_index(
    index: usize,
    state: State<'_, DesktopState>,
) -> Result<AppState, String> {
    let client = management_client(&state, "无法按序号删除 API key")?;
    client
        .delete_api_key_by_index(index)
        .await
        .map_err(|error| error.to_string())?;
    refresh_snapshot_with_client(&state, client, "无法刷新 API key 删除后的状态").await
}

#[tauri::command]
async fn set_management_auth_file_disabled(
    name: String,
    disabled: bool,
    state: State<'_, DesktopState>,
) -> Result<AppState, String> {
    let client = management_client(&state, "无法更新账号状态")?;
    client
        .set_auth_file_disabled(name, disabled)
        .await
        .map_err(|error| error.to_string())?;
    refresh_snapshot_with_client(&state, client, "无法刷新账号状态更新后的状态").await
}

#[tauri::command]
async fn delete_management_auth_file(
    name: String,
    state: State<'_, DesktopState>,
) -> Result<AppState, String> {
    let client = management_client(&state, "无法删除账号")?;
    client
        .delete_auth_file(&name)
        .await
        .map_err(|error| error.to_string())?;
    refresh_snapshot_with_client(&state, client, "无法刷新账号删除后的状态").await
}

#[tauri::command]
async fn delete_all_management_auth_files(
    state: State<'_, DesktopState>,
) -> Result<AppState, String> {
    let client = management_client(&state, "无法删除全部账号")?;
    client
        .delete_all_auth_files()
        .await
        .map_err(|error| error.to_string())?;
    refresh_snapshot_with_client(&state, client, "无法刷新账号清理后的状态").await
}

#[tauri::command]
async fn start_management_oauth(
    endpoint: String,
    project_id: Option<String>,
    is_webui: bool,
    state: State<'_, DesktopState>,
) -> Result<OAuthUrlResponse, String> {
    let client = management_client(&state, "无法启动 OAuth")?;
    client
        .get_oauth_url(&endpoint, project_id.as_deref(), is_webui)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn poll_management_oauth(
    token: String,
    state: State<'_, DesktopState>,
) -> Result<OAuthStatusResponse, String> {
    let client = management_client(&state, "无法轮询 OAuth 状态")?;
    client
        .poll_oauth_status(&token)
        .await
        .map_err(|error| error.to_string())
}

/// Manually complete an OAuth login by replaying the pasted callback URL
/// (http://localhost:1455/auth/callback?code=...&state=...) to the proxy's local
/// listener from the native side, bypassing any browser/system proxy on loopback.
#[tauri::command]
fn submit_oauth_callback(url: String) -> Result<(), String> {
    quotio_core::submit_oauth_callback(&url)
}

#[tauri::command]
async fn import_management_vertex_service_account(
    json: String,
    state: State<'_, DesktopState>,
) -> Result<AppState, String> {
    let client = management_client(&state, "无法导入 Vertex service account")?;
    client
        .upload_vertex_service_account(json)
        .await
        .map_err(|error| error.to_string())?;
    refresh_snapshot_with_client(&state, client, "无法刷新 Vertex 导入后的状态").await
}

#[tauri::command]
async fn set_management_max_retry_interval(
    seconds: u16,
    state: State<'_, DesktopState>,
) -> Result<AppState, String> {
    let client = management_client(&state, "无法写入最大重试间隔")?;
    client
        .set_max_retry_interval(seconds)
        .await
        .map_err(|error| error.to_string())?;
    refresh_snapshot_with_client(&state, client, "无法刷新最大重试间隔写入后的状态").await
}

#[tauri::command]
async fn set_management_logging_to_file(
    enabled: bool,
    state: State<'_, DesktopState>,
) -> Result<AppState, String> {
    let client = management_client(&state, "无法写入文件日志开关")?;
    client
        .set_logging_to_file(enabled)
        .await
        .map_err(|error| error.to_string())?;
    refresh_snapshot_with_client(&state, client, "无法刷新文件日志写入后的状态").await
}

#[tauri::command]
async fn set_management_quota_switch_project(
    enabled: bool,
    state: State<'_, DesktopState>,
) -> Result<AppState, String> {
    let client = management_client(&state, "无法写入超额切换项目开关")?;
    client
        .set_quota_exceeded_switch_project(enabled)
        .await
        .map_err(|error| error.to_string())?;
    refresh_snapshot_with_client(&state, client, "无法刷新超额切换项目写入后的状态").await
}

#[tauri::command]
async fn set_management_quota_switch_preview_model(
    enabled: bool,
    state: State<'_, DesktopState>,
) -> Result<AppState, String> {
    let client = management_client(&state, "无法写入超额切换预览模型开关")?;
    client
        .set_quota_exceeded_switch_preview_model(enabled)
        .await
        .map_err(|error| error.to_string())?;
    refresh_snapshot_with_client(&state, client, "无法刷新超额切换预览模型写入后的状态").await
}

fn management_client(
    state: &State<'_, DesktopState>,
    lock_error: &str,
) -> Result<ManagementApiClient, String> {
    let mut core = state.core.lock().map_err(|_| lock_error.to_string())?;
    core.management_client().map_err(|error| error.to_string())
}

fn apply_management_snapshot(
    state: &State<'_, DesktopState>,
    snapshot: ManagementSnapshot,
    lock_error: &str,
) -> Result<AppState, String> {
    let mut core = state.core.lock().map_err(|_| lock_error.to_string())?;
    Ok(core.apply_management_snapshot(snapshot))
}

async fn refresh_snapshot_with_client(
    state: &State<'_, DesktopState>,
    client: ManagementApiClient,
    lock_error: &str,
) -> Result<AppState, String> {
    let snapshot = client
        .refresh_snapshot()
        .await
        .map_err(|error| error.to_string())?;
    apply_management_snapshot(state, snapshot, lock_error)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
/// Position the menu-bar panel at the top-right of the primary monitor, then
/// show + focus it.
fn position_menubar(panel: &tauri::WebviewWindow) {
    if let Ok(Some(monitor)) = panel.primary_monitor() {
        let scale = monitor.scale_factor();
        let screen_w = monitor.size().width as f64 / scale;
        let x = (screen_w - 280.0 - 12.0).max(0.0);
        let _ = panel.set_position(tauri::LogicalPosition::new(x, 20.0));
    }
    let _ = panel.show();
    let _ = panel.set_focus();
}

/// Show/hide the tray "menu bar" quota panel, anchored to the top-right corner.
fn toggle_menubar(app: &AppHandle) {
    let Some(panel) = app.get_webview_window("menubar") else {
        return;
    };
    if panel.is_visible().unwrap_or(false) {
        let _ = panel.hide();
    } else {
        position_menubar(&panel);
    }
}

/// Poll interval for the background usage collector (ms). Comfortably under the
/// proxy's default 60s queue retention so events are not lost at desktop volume.
const USAGE_COLLECTOR_POLL_MS: u64 = 1500;

/// Spawn the single background consumer of the proxy's destructive `/usage-queue`.
/// It drains events at high frequency and persists them to the usage store, so
/// the dashboard can aggregate history across arbitrary time ranges. Runs for
/// the life of the process; iterations are cheap no-ops while the proxy is down.
fn spawn_usage_collector(app: AppHandle) {
    std::thread::spawn(move || {
        // Let the proxy / management endpoint come up before the first drain.
        std::thread::sleep(std::time::Duration::from_secs(3));
        let mut tick: u64 = 0;
        loop {
            let prepared = app.try_state::<DesktopState>().and_then(|state| {
                let mut core = state.core.lock().ok()?;
                let client = core.management_client().ok()?;
                Some((client, core.usage_store()))
            });
            if let Some((client, store)) = prepared {
                // Re-assert usage telemetry every ~30s so the queue keeps filling
                // even when nothing else refreshes the management snapshot.
                if tick % 20 == 0 {
                    let _ =
                        tauri::async_runtime::block_on(client.set_usage_statistics_enabled(true));
                }
                let events = tauri::async_runtime::block_on(client.fetch_usage_events(2000))
                    .unwrap_or_default();
                if !events.is_empty() {
                    let inserted = store.insert_events(&events);
                    if inserted > 0 {
                        let _ = app.emit("usage-updated", inserted);
                    }
                }
            }
            tick = tick.wrapping_add(1);
            std::thread::sleep(std::time::Duration::from_millis(USAGE_COLLECTOR_POLL_MS));
        }
    });
}

pub fn run() {
    #[allow(unused_mut)]
    let mut builder = tauri::Builder::default();
    // Single-instance must be registered first: a second launch is forwarded to
    // the running instance (which we focus / restore from the tray) and then
    // exits, so the app never opens twice.
    #[cfg(desktop)]
    {
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
        }));
    }
    builder
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_notification::init())
        .manage(DesktopState {
            core: Arc::new(Mutex::new(AppCore::default())),
            tunnel: Mutex::new(TunnelRuntime::default()),
        })
        .setup(|app| {
            if let Ok(proxy_resource_root) = app
                .path()
                .resolve("resources/proxy", tauri::path::BaseDirectory::Resource)
            {
                quotio_platform::set_proxy_resource_root(proxy_resource_root);
            }

            let show = MenuItem::with_id(app, "show", "打开 Quotio", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show, &quit])?;

            let mut tray_builder = TrayIconBuilder::with_id("main-tray")
                .tooltip("Quotio")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "show" => {
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                    "quit" => app.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        toggle_menubar(tray.app_handle());
                    }
                });
            // Tauri's tray shows nothing without an explicit icon. Reuse the
            // app's embedded window icon so the tray is visible on Windows.
            if let Some(icon) = app.default_window_icon().cloned() {
                tray_builder = tray_builder.icon(icon);
            }
            tray_builder.build(app)?;

            spawn_usage_collector(app.handle().clone());

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_app_state,
            get_platform_info,
            save_settings,
            update_fallback_configuration,
            detect_agents,
            read_agent_configuration,
            configure_agent,
            list_agent_backups,
            restore_agent_backup,
            reset_agent_configuration,
            detect_codex_app,
            list_codex_launch_accounts,
            codex_start,
            codex_stop,
            codex_launch_active,
            fetch_codex_models,
            discover_available_models,
            refresh_fallback_route_state,
            credential_status,
            clear_remote_management_key,
            open_config_root,
            set_launch_at_login,
            request_notification_permission,
            send_test_notification,
            start_proxy,
            stop_proxy,
            restart_proxy,
            download_proxy_binary,
            check_proxy_health,
            refresh_management_state,
            drain_request_logs,
            query_usage_stats,
            query_account_summary,
            query_usage_timeseries,
            query_usage_model_breakdown,
            list_usage_filter_options,
            query_account_auth_health,
            get_model_prices,
            set_model_prices,
            tunnel_status,
            download_cloudflared,
            start_tunnel,
            stop_tunnel,
            warmup_accounts,
            get_autostart,
            set_autostart,
            show_main_window,
            quit_app,
            show_menubar,
            import_auth_file,
            list_local_accounts,
            list_custom_providers,
            add_custom_provider,
            delete_custom_provider,
            update_custom_provider,
            list_api_keys,
            add_api_key,
            remove_api_key,
            update_api_key,
            refresh_quotas,
            get_management_debug,
            set_management_debug,
            get_management_routing_strategy,
            set_management_routing_strategy,
            get_management_proxy_url,
            set_management_proxy_url,
            clear_management_proxy_url,
            set_management_request_log,
            set_management_request_retry,
            clear_management_logs,
            add_management_api_key,
            update_management_api_key,
            delete_management_api_key,
            delete_management_api_key_by_index,
            set_management_auth_file_disabled,
            delete_management_auth_file,
            delete_all_management_auth_files,
            start_management_oauth,
            poll_management_oauth,
            submit_oauth_callback,
            import_management_vertex_service_account,
            set_management_max_retry_interval,
            set_management_logging_to_file,
            set_management_quota_switch_project,
            set_management_quota_switch_preview_model
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app_handle, event| {
            if let tauri::RunEvent::Exit = event {
                // Release resources on every exit path (close dialog / tray / menu
                // bar): stop a Quotio-started proxy + the tunnel, and terminate the
                // adopted/external proxy by its port so it doesn't linger.
                if let Some(state) = app_handle.try_state::<DesktopState>() {
                    if let Ok(mut core) = state.core.lock() {
                        core.shutdown();
                    }
                    if let Ok(mut tunnel) = state.tunnel.lock() {
                        if let Some(mut child) = tunnel.child.take() {
                            let _ = child.kill();
                            let _ = child.wait();
                        }
                    }
                }
            }
        });
}
