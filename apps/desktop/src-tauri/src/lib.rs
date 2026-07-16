use std::sync::{Arc, Mutex, MutexGuard};

use quotio_core::{
    management::{ManagementApiClient, ManagementApiError},
    AppCore,
};
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
    utils::config::WindowEffectsConfig,
    window::Effect,
    AppHandle, Emitter, Manager, State,
};
use tauri_plugin_autostart::ManagerExt;

struct DesktopState {
    core: Arc<Mutex<AppCore>>,
    tunnel: Mutex<TunnelRuntime>,
}

/// 取 core 锁,毒化时恢复并清除毒化标记。
/// std `Mutex` 默认在某个持锁线程 panic 后被「毒化」,之后每次 `lock()` 都返回 Err——
/// 若命令面直接把它当错误返回,一次 panic 就会让所有 IPC 命令永久失败、整个 UI 砖化。
/// 这里统一用 `into_inner()` 拿回数据继续用,并 `clear_poison()` 让后续 `lock()`(含后台
/// collector 的 best-effort `.ok()`)恢复正常,避免毒化长期残留。
fn lock_core(core: &Mutex<AppCore>) -> MutexGuard<'_, AppCore> {
    core.lock().unwrap_or_else(|poisoned| {
        eprintln!("[lib] core mutex poisoned — 已恢复并清除毒化标记");
        core.clear_poison();
        poisoned.into_inner()
    })
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
async fn get_app_state(state: State<'_, DesktopState>) -> Result<AppState, String> {
    // app_state() probes the proxy health (a sync network call) and reads every
    // auth file in the auth dir — slow with many accounts. Run it on a blocking
    // worker so the UI/main thread never freezes on startup or refresh.
    let core = state.core.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut core = core.lock().map_err(|_| "无法读取应用状态".to_string())?;
        Ok(core.app_state())
    })
    .await
    .map_err(|error| format!("读取应用状态任务异常：{error}"))?
}

#[tauri::command]
fn get_platform_info(state: State<'_, DesktopState>) -> Result<PlatformInfo, String> {
    let core = lock_core(&state.core);
    Ok(core.platform_info())
}

#[tauri::command]
fn save_settings(
    app: AppHandle,
    settings: AppSettings,
    allow_clear_codex_profiles: Option<bool>,
    state: State<'_, DesktopState>,
) -> Result<AppState, String> {
    let mut core = lock_core(&state.core);
    let result = core
        .save_settings(settings, allow_clear_codex_profiles.unwrap_or(false))
        .map_err(|error| error.to_string())?;
    // 调度规则可能刚被开/关：立即收敛池子（开→接管，关→还原 standby 账号）。
    if core.scheduler_reconcile() {
        let _ = app.emit("scheduler-changed", ());
        return Ok(core.app_state());
    }
    Ok(result)
}

#[tauri::command]
fn update_fallback_configuration(
    action: FallbackConfigAction,
    state: State<'_, DesktopState>,
) -> Result<AppState, String> {
    let mut core = lock_core(&state.core);
    core.update_fallback_configuration(action)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn detect_agents(state: State<'_, DesktopState>) -> Result<AppState, String> {
    let mut core = lock_core(&state.core);
    Ok(core.detect_agents())
}

#[tauri::command]
fn read_agent_configuration(
    agent_id: String,
    state: State<'_, DesktopState>,
) -> Result<SavedAgentConfiguration, String> {
    let core = lock_core(&state.core);
    core.read_agent_configuration(&agent_id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn configure_agent(
    request: AgentConfigurationRequest,
) -> Result<AgentConfigurationResult, String> {
    // 配置生成可能扫描 Codex 安装并执行文件 I/O，移到阻塞线程池避免占用 IPC/UI 线程。
    tauri::async_runtime::spawn_blocking(move || configure_agent_blocking(request))
        .await
        .map_err(|error| format!("配置智能体任务异常：{error}"))?
}

fn configure_agent_blocking(
    request: AgentConfigurationRequest,
) -> Result<AgentConfigurationResult, String> {
    quotio_core::agent_config::configure_agent(request).map_err(|error| error.to_string())
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

#[tauri::command]
fn list_dream_skin_themes() -> Result<Vec<quotio_core::dream_skin::DreamSkinThemeSummary>, String> {
    quotio_core::dream_skin::list_themes()
}

#[tauri::command]
async fn import_dream_skin_theme(
    image_path: String,
    name: Option<String>,
) -> Result<quotio_core::dream_skin::DreamSkinThemeSummary, String> {
    tauri::async_runtime::spawn_blocking(move || {
        quotio_core::dream_skin::import_theme(std::path::Path::new(&image_path), name.as_deref())
    })
    .await
    .map_err(|error| format!("导入 Dream Skin 主题任务异常：{error}"))?
}

/// 某个 Codex 模型支持的推理档位(从本机 codex 二进制内置的模型目录里读)。
/// 二进制扫描可能较慢，放到 blocking 线程跑，避免卡住 IPC / UI 线程。
#[tauri::command]
async fn fetch_codex_reasoning_levels(model: String) -> Result<Vec<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        quotio_core::codex_catalog::reasoning_levels_result(&model)
    })
    .await
    .map_err(|error| format!("读取 Codex 模型目录任务异常：{}", error))?
}

/// 一键启动指定方案的 Codex：确保代理 → 备份原始 → 写配置 → 注入账号 → 启动。
/// 全程都是阻塞活儿（关进程 / 写文件 / 修 sqlite / 拉起进程，商店版还要轮询 pid 数秒），
/// 放到 blocking 线程跑，避免冻住 UI 主线程（否则窗口会「未响应」）。
#[tauri::command]
async fn codex_start(state: State<'_, DesktopState>, profile_id: String) -> Result<String, String> {
    let core = Arc::clone(&state.core);
    tauri::async_runtime::spawn_blocking(move || {
        let mut core = core.lock().map_err(|_| "无法启动 Codex".to_string())?;
        core.codex_start(&profile_id)
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| format!("启动任务异常：{}", error))?
}

/// 停止 Codex：杀掉启动的进程 + 把 ~/.codex 还原到启动前。同样放到 blocking 线程,避免冻 UI。
#[tauri::command]
async fn codex_stop(state: State<'_, DesktopState>) -> Result<String, String> {
    let core = Arc::clone(&state.core);
    tauri::async_runtime::spawn_blocking(move || {
        let mut core = core.lock().map_err(|_| "无法停止 Codex".to_string())?;
        core.codex_stop().map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| format!("停止任务异常：{}", error))?
}

/// 当前 Codex 一键启动会话是否在运行。
#[tauri::command]
fn codex_launch_active(state: State<'_, DesktopState>) -> Result<bool, String> {
    let core = lock_core(&state.core);
    Ok(core.codex_active())
}

/// 当前在跑的启动方案 id（前端据此高亮「运行中」那套；无则 null）。
#[tauri::command]
fn codex_active_profile(state: State<'_, DesktopState>) -> Result<Option<String>, String> {
    let core = lock_core(&state.core);
    Ok(core.active_codex_profile_id())
}

/// quotio-key-router 插件是否就位。没有它,「按 key 绑定服务商」就不会生成路由配置、绑定不生效
/// (代理仍全局轮询命中所有池)。前端据此给「绑了 key 却不隔离」做防呆警告。
#[tauri::command]
fn key_router_available(state: State<'_, DesktopState>) -> bool {
    state
        .core
        .lock()
        .map(|core| core.key_router_plugin_staged())
        .unwrap_or(false)
}

/// 修复 Codex 历史会话可见性：对齐 rollout/state_5.sqlite 里的 provider 元数据。
#[tauri::command]
async fn codex_repair_session_visibility() -> Result<String, String> {
    let summary = tauri::async_runtime::spawn_blocking(|| {
        quotio_core::codex_session_visibility::repair_session_visibility_in_default_dir()
    })
    .await
    .map_err(|error| format!("修复任务异常：{}", error))??;
    Ok(summary.message)
}

/// 拉取代理实际服务的 codex 模型（前端模型下拉用）。best-effort：拿不到返回空，前端回退内置列表。
#[tauri::command]
fn fetch_codex_models(state: State<'_, DesktopState>) -> Result<Vec<String>, String> {
    let (endpoint, api_key) = {
        let core = lock_core(&state.core);
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
    let core = lock_core(&state.core);
    core.list_agent_backups(&agent_id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn restore_agent_backup(
    agent_id: String,
    backup_path: String,
) -> Result<AgentConfigurationResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        quotio_core::agent_config::restore_agent_backup(&agent_id, &backup_path)
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| format!("恢复智能体备份任务异常：{error}"))?
}

#[tauri::command]
async fn reset_agent_configuration(agent_id: String) -> Result<AgentConfigurationResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        quotio_core::agent_config::reset_agent_configuration(&agent_id)
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| format!("重置智能体配置任务异常：{error}"))?
}

#[tauri::command]
fn discover_available_models(
    state: State<'_, DesktopState>,
) -> Result<Vec<AvailableModel>, String> {
    let mut core = lock_core(&state.core);
    Ok(core
        .discover_available_models()
        .fallback_runtime
        .available_models)
}

#[tauri::command]
fn refresh_fallback_route_state(state: State<'_, DesktopState>) -> Result<AppState, String> {
    let mut core = lock_core(&state.core);
    Ok(core.refresh_fallback_route_state())
}

#[tauri::command]
fn credential_status(state: State<'_, DesktopState>) -> Result<CredentialStatus, String> {
    let core = lock_core(&state.core);
    Ok(core.credential_status())
}

#[tauri::command]
fn clear_remote_management_key(state: State<'_, DesktopState>) -> Result<AppState, String> {
    let mut core = lock_core(&state.core);
    core.clear_remote_management_key()
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn open_config_root(state: State<'_, DesktopState>) -> Result<(), String> {
    let core = lock_core(&state.core);
    core.open_config_root().map_err(|error| error.to_string())
}

#[tauri::command]
fn open_logs_dir() -> Result<(), String> {
    quotio_core::open_logs_dir()
}

#[tauri::command]
async fn set_launch_at_login(
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
    let resolved = app.autolaunch().is_enabled().unwrap_or(enabled);

    // app_state()/save_settings 内含同步健康探测(对不可达端点最坏阻塞数秒)。放到
    // spawn_blocking,别在 Tauri 主线程上跑、冻住 UI(与 start/stop_proxy 一致)。
    let core = Arc::clone(&state.core);
    tauri::async_runtime::spawn_blocking(move || {
        let mut core = lock_core(&core);
        let mut settings = core.app_state().settings;
        settings.launch_at_login = resolved;
        core.save_settings(settings, false)
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| format!("保存自启状态任务异常：{}", error))?
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
    let mut core = lock_core(&state.core);
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
    let (dest, proxy_url) = {
        let core = lock_core(&state.core);
        (core.proxy_managed_binary_path(), core.proxy_upstream_url())
    };

    let progress_app = app.clone();
    let tag = tauri::async_runtime::spawn_blocking(move || {
        let mut last_percent = u8::MAX;
        quotio_core::proxy_download::download_proxy_binary(
            &dest,
            proxy_url.as_deref(),
            |downloaded, total| {
                if total == 0 {
                    return;
                }
                let percent = (downloaded.saturating_mul(100) / total).min(100) as u8;
                if percent != last_percent {
                    last_percent = percent;
                    let _ = progress_app.emit("proxy-download-progress", percent);
                }
            },
        )
    })
    .await
    .map_err(|error| format!("下载任务异常：{}", error))??;

    let _ = app.emit("proxy-download-progress", 100u8);
    let mut core = lock_core(&state.core);
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
    let (dest, proxy_url) = {
        let core = lock_core(&state.core);
        (core.cloudflared_binary_path(), core.proxy_upstream_url())
    };
    let progress_app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut last_percent = u8::MAX;
        quotio_core::tunnel::download_cloudflared(
            &dest,
            proxy_url.as_deref(),
            |downloaded, total| {
                if total == 0 {
                    return;
                }
                let percent = (downloaded.saturating_mul(100) / total).min(100) as u8;
                if percent != last_percent {
                    last_percent = percent;
                    let _ = progress_app.emit("cloudflared-download-progress", percent);
                }
            },
        )
    })
    .await
    .map_err(|error| format!("下载任务异常：{}", error))??;
    let _ = app.emit("cloudflared-download-progress", 100u8);
    Ok(current_tunnel_status(&state))
}

#[tauri::command]
fn start_tunnel(app: AppHandle, state: State<'_, DesktopState>) -> Result<TunnelStatus, String> {
    let (binary, port) = {
        let core = lock_core(&state.core);
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
        position_menubar(&panel, None);
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
async fn check_proxy_health(state: State<'_, DesktopState>) -> Result<AppState, String> {
    // 健康检查本就做同步网络探测(最坏阻塞数秒):放到 spawn_blocking,别冻 UI。
    let core = Arc::clone(&state.core);
    tauri::async_runtime::spawn_blocking(move || {
        let mut core = lock_core(&core);
        core.check_proxy_health()
    })
    .await
    .map_err(|error| format!("健康检查任务异常：{}", error))
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
        let mut core = lock_core(&state.core);
        return Ok(core.app_state());
    };
    let _ = client.set_usage_statistics_enabled(true).await;
    let events = client.fetch_usage_events(2000).await.unwrap_or_default();
    let store = {
        let core = lock_core(&state.core);
        core.usage_store()
    };
    if !events.is_empty() {
        store.insert_events(&events);
        quotio_core::append_request_errors(&events);
    }
    let mut core = lock_core(&state.core);
    Ok(core.app_state())
}

#[tauri::command]
fn query_usage_stats(
    query: UsageQuery,
    state: State<'_, DesktopState>,
) -> Result<UsageAggregate, String> {
    let core = lock_core(&state.core);
    Ok(core.query_usage_stats(&query))
}

#[tauri::command]
fn query_account_summary(
    query: UsageQuery,
    state: State<'_, DesktopState>,
) -> Result<Vec<AccountSummaryRow>, String> {
    let core = lock_core(&state.core);
    Ok(core.query_account_summary(&query))
}

#[tauri::command]
fn query_usage_timeseries(
    query: UsageQuery,
    bucket: UsageChartBucket,
    state: State<'_, DesktopState>,
) -> Result<Vec<UsageTimeSeriesPoint>, String> {
    let core = lock_core(&state.core);
    Ok(core.query_usage_timeseries(&query, bucket))
}

#[tauri::command]
fn query_usage_model_breakdown(
    query: UsageQuery,
    limit: Option<usize>,
    state: State<'_, DesktopState>,
) -> Result<Vec<UsageModelBreakdownRow>, String> {
    let core = lock_core(&state.core);
    Ok(core.query_usage_model_breakdown(&query, limit.unwrap_or(10)))
}

#[tauri::command]
fn list_usage_filter_options(state: State<'_, DesktopState>) -> Result<UsageFilterOptions, String> {
    let core = lock_core(&state.core);
    Ok(core.usage_filter_options())
}

#[tauri::command]
fn query_account_auth_health(
    state: State<'_, DesktopState>,
) -> Result<Vec<AccountAuthHealth>, String> {
    let core = lock_core(&state.core);
    Ok(core.account_auth_health())
}

#[tauri::command]
fn get_model_prices(state: State<'_, DesktopState>) -> Result<Vec<ModelPrice>, String> {
    let core = lock_core(&state.core);
    Ok(core.model_prices())
}

#[tauri::command]
fn set_model_prices(
    prices: Vec<ModelPrice>,
    state: State<'_, DesktopState>,
) -> Result<Vec<ModelPrice>, String> {
    let core = lock_core(&state.core);
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
    let mut core = lock_core(&state.core);
    core.import_auth_file(&filename, &content)
}

/// Export CPA account credential files as one zip. `names` (auth file names of a
/// single provider) limits the export to those accounts; omit it to export all.
/// Returns the zip's path so the UI can reveal it in the file manager.
#[tauri::command]
fn export_auth_files(path: String, names: Option<Vec<String>>) -> Result<String, String> {
    quotio_core::export_auth_files(std::path::Path::new(&path), names.as_deref())
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
    models: String,
    proxy_mode: String,
    state: State<'_, DesktopState>,
) -> Result<Vec<quotio_core::CustomProvider>, String> {
    let result = quotio_core::add_custom_provider(
        name, base_url, api_key, kind, prefix, models, proxy_mode,
    )?;
    let core = lock_core(&state.core);
    core.rewrite_proxy_config();
    Ok(result)
}

#[tauri::command]
fn delete_custom_provider(
    id: String,
    state: State<'_, DesktopState>,
) -> Result<Vec<quotio_core::CustomProvider>, String> {
    let result = quotio_core::delete_custom_provider(&id)?;
    let core = lock_core(&state.core);
    core.rewrite_proxy_config();
    Ok(result)
}

#[tauri::command]
fn update_custom_provider(
    id: String,
    name: String,
    base_url: String,
    api_key: String,
    kind: String,
    prefix: String,
    models: String,
    proxy_mode: String,
    state: State<'_, DesktopState>,
) -> Result<Vec<quotio_core::CustomProvider>, String> {
    let result = quotio_core::update_custom_provider(
        id, name, base_url, api_key, kind, prefix, models, proxy_mode,
    )?;
    let core = lock_core(&state.core);
    core.rewrite_proxy_config();
    Ok(result)
}

#[tauri::command]
fn add_provider_key(
    provider_id: String,
    label: String,
    api_key: String,
    state: State<'_, DesktopState>,
) -> Result<Vec<quotio_core::CustomProvider>, String> {
    let result = quotio_core::add_provider_key(&provider_id, label, api_key)?;
    let core = lock_core(&state.core);
    core.rewrite_proxy_config();
    Ok(result)
}

#[tauri::command]
fn remove_provider_key(
    provider_id: String,
    key_id: String,
    state: State<'_, DesktopState>,
) -> Result<Vec<quotio_core::CustomProvider>, String> {
    let result = quotio_core::remove_provider_key(&provider_id, &key_id)?;
    let core = lock_core(&state.core);
    core.rewrite_proxy_config();
    Ok(result)
}

#[tauri::command]
fn toggle_provider_key(
    provider_id: String,
    key_id: String,
    state: State<'_, DesktopState>,
) -> Result<Vec<quotio_core::CustomProvider>, String> {
    let result = quotio_core::toggle_provider_key(&provider_id, &key_id)?;
    let core = lock_core(&state.core);
    core.rewrite_proxy_config();
    Ok(result)
}

#[tauri::command]
fn list_api_keys() -> Vec<String> {
    quotio_core::get_api_keys()
}

#[tauri::command]
fn get_api_key_bindings() -> Vec<quotio_types::ApiKeyBinding> {
    quotio_core::get_api_key_bindings()
}

#[tauri::command]
fn set_api_key_binding(
    api_key: String,
    provider_id: String,
    state: State<'_, DesktopState>,
) -> Result<Vec<quotio_types::ApiKeyBinding>, String> {
    let result = quotio_core::set_api_key_binding(api_key, provider_id)?;
    let core = lock_core(&state.core);
    core.rewrite_proxy_config();
    Ok(result)
}

#[tauri::command]
async fn add_api_key(key: String, state: State<'_, DesktopState>) -> Result<AppState, String> {
    quotio_core::add_api_key(key)?;
    let core = Arc::clone(&state.core);
    tauri::async_runtime::spawn_blocking(move || {
        let mut core = lock_core(&core);
        core.rewrite_proxy_config();
        core.app_state()
    })
    .await
    .map_err(|error| format!("新增密钥任务异常：{}", error))
}

#[tauri::command]
async fn remove_api_key(key: String, state: State<'_, DesktopState>) -> Result<AppState, String> {
    quotio_core::remove_api_key(&key)?;
    let core = Arc::clone(&state.core);
    tauri::async_runtime::spawn_blocking(move || {
        let mut core = lock_core(&core);
        core.rewrite_proxy_config();
        core.app_state()
    })
    .await
    .map_err(|error| format!("删除密钥任务异常：{}", error))
}

#[tauri::command]
async fn update_api_key(
    key: String,
    replacement: String,
    state: State<'_, DesktopState>,
) -> Result<AppState, String> {
    quotio_core::update_api_key(&key, replacement)?;
    let core = Arc::clone(&state.core);
    tauri::async_runtime::spawn_blocking(move || {
        let mut core = lock_core(&core);
        core.rewrite_proxy_config();
        core.app_state()
    })
    .await
    .map_err(|error| format!("替换密钥任务异常：{}", error))
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
        let core = lock_core(&state.core);
        // Clean up duplicate same-account files before listing (re-import / re-login
        // can leave two files = two cards); keeps the bound / newest one.
        core.dedup_codex_auth();
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

    let mut core = lock_core(&state.core);
    core.store_quotas(quotas);
    // 配额刷新后跑一轮智能调度（规则关闭时它负责把 standby 账号放回池子）。
    if core.scheduler_reconcile() {
        let _ = app.emit("scheduler-changed", ());
    }
    Ok(core.app_state())
}

/// Spend one Codex "主动重置次数" (rate-limit reset credit) for the given account,
/// force-resetting its 5h window. The frontend refreshes quotas afterward to pick
/// up the new state. Errors carry a localized message for inline display.
#[tauri::command]
async fn consume_codex_reset_credit(
    account_key: String,
    state: State<'_, DesktopState>,
) -> Result<(), String> {
    let proxy_url = {
        let core = lock_core(&state.core);
        core.proxy_upstream_url()
    };
    tauri::async_runtime::spawn_blocking(move || {
        quotio_core::quota::consume_codex_reset_credit(&account_key, proxy_url.as_deref())
    })
    .await
    .map_err(|error| format!("重置任务异常：{}", error))?
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
    let url = client
        .get_proxy_url()
        .await
        .map_err(|error| error.to_string())?;
    if !url.is_empty() {
        return Ok(url);
    }
    let mut core = lock_core(&state.core);
    let config_url = core
        .app_state()
        .management
        .config
        .as_ref()
        .and_then(|c| c.proxy_url.clone())
        .unwrap_or_default();
    Ok(config_url)
}

#[tauri::command]
async fn set_management_proxy_url(
    url: String,
    state: State<'_, DesktopState>,
) -> Result<AppState, String> {
    let client = management_client(&state, "无法写入代理 URL")?;
    match client.set_proxy_url(url.clone()).await {
        Ok(()) => {}
        // 该版本管理接口没有运行时 /proxy-url 端点:静默降级——proxy_url 已随设置写入
        // config.yaml,代理热重载会读到,无需报错惊扰用户。
        Err(ManagementApiError::Status(404)) => {
            eprintln!("[set_management_proxy_url] 运行时端点不可用(404),已保存到配置,重载生效");
        }
        // 代理拒绝该地址(最常见:上游代理不可达/没有服务在监听,或值无效)。给清晰提示
        // 而非裸 HTTP 400;设置已写进 config.yaml,但提醒用户确认该地址确实在跑。
        Err(ManagementApiError::Status(400)) => {
            return Err(format!(
                "代理拒绝了上游地址「{}」——通常是该地址不可达(没有服务在监听)或格式无效。设置已保存到配置,但请确认该代理确实在运行。",
                url.trim()
            ));
        }
        Err(error) => return Err(error.to_string()),
    }
    refresh_snapshot_with_client(&state, client, "无法刷新代理 URL 写入后的状态").await
}

#[tauri::command]
async fn clear_management_proxy_url(state: State<'_, DesktopState>) -> Result<AppState, String> {
    let client = management_client(&state, "无法清空代理 URL")?;
    match client.delete_proxy_url().await {
        Ok(()) => {}
        // 运行时端点不可用(版本不支持):静默降级,config.yaml 已按设置清空、重载生效。
        Err(ManagementApiError::Status(400 | 404)) => {
            eprintln!("[clear_management_proxy_url] 运行时端点不可用,已按设置清空、重载生效");
        }
        Err(error) => return Err(error.to_string()),
    }
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

/// 清空「请求」日志(usage_store / SQLite)。日志页「请求」tab 删除调用——它和代理
/// 文本日志(clear_management_logs)是两份不同数据,之前删除按钮只清后者,导致在
/// 「请求」tab 点删除看着没反应。纯本地 SQLite 操作,放 spawn_blocking 不阻塞 UI。
#[tauri::command]
async fn clear_request_logs(
    app: AppHandle,
    state: State<'_, DesktopState>,
) -> Result<AppState, String> {
    let core = Arc::clone(&state.core);
    let result = tauri::async_runtime::spawn_blocking(move || {
        let mut core = lock_core(&core);
        core.clear_request_logs()
    })
    .await
    .map_err(|error| format!("清空请求日志任务异常：{}", error))?;
    // 仪表盘与请求日志同源,通知它刷新。
    let _ = app.emit("usage-updated", 0u64);
    Ok(result)
}

/// 请求日志总条数。日志页删除按钮在弹二次确认前调用,告知用户实际会删多少条
/// (列表只显示最近 500,实际存储远多于此,必须如实告知以免误删全部历史)。
#[tauri::command]
async fn count_request_logs(state: State<'_, DesktopState>) -> Result<usize, String> {
    let core = Arc::clone(&state.core);
    tauri::async_runtime::spawn_blocking(move || {
        let core = lock_core(&core);
        core.count_request_logs()
    })
    .await
    .map_err(|error| format!("统计请求日志任务异常：{}", error))
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
        .set_auth_file_disabled(name.clone(), disabled)
        .await
        .map_err(|error| error.to_string())?;
    if !disabled {
        // 用户手动启用:彻底清掉调度待命 + 健康隔离两个临时标记并放回池子,
        // 否则残留的 standby 会把刚启用的账号又写回 disabled=true(启用静默失效)。
        quotio_core::scheduler::clear_temp_disable_markers_for_file_in(
            &quotio_platform::proxy_auth_dir(),
            &name,
        );
    }
    refresh_snapshot_with_client(&state, client, "无法刷新账号状态更新后的状态").await
}

/// 调整某服务商账号的请求顺序:按 `ordered_file_names` 写 quotio_priority=1..N
/// (空列表 = 重置为自动顺序),随即重跑一轮调度让激活号按新优先级更新。
#[tauri::command]
async fn reorder_provider_accounts(
    provider_id: String,
    ordered_file_names: Vec<String>,
    state: State<'_, DesktopState>,
) -> Result<AppState, String> {
    let core = Arc::clone(&state.core);
    tauri::async_runtime::spawn_blocking(move || {
        let dir = quotio_platform::proxy_auth_dir();
        let mut core = lock_core(&core);
        quotio_core::scheduler::reorder_provider_in(&dir, &provider_id, &ordered_file_names);
        core.scheduler_reconcile();
        core.app_state()
    })
    .await
    .map_err(|error| format!("调整账号顺序任务异常：{}", error))
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
    // Codex 登录用本地 http://localhost:1455/auth/callback 接码；若 1455 绑不上
    // （保留排除区间 / 被占），浏览器回调会失败、登录静默卡住——提前给明确提示。
    if endpoint.to_lowercase().contains("codex") {
        quotio_core::probe_codex_oauth_port()?;
    }
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
    let response = client
        .poll_oauth_status(&token)
        .await
        .map_err(|error| error.to_string())?;
    // On a completed login CLIProxyAPI just wrote a fresh auth file; if it's a
    // re-login of an existing account (under a different filename) this dedups so
    // we don't end up with two files / two cards for the same account.
    if matches!(
        response.status.to_ascii_lowercase().as_str(),
        "ok" | "success" | "completed"
    ) {
        if let Ok(core) = state.core.lock() {
            core.dedup_codex_auth();
        }
    }
    Ok(response)
}

/// Manually complete an OAuth login by replaying the pasted callback URL
/// (http://localhost:1455/auth/callback?code=...&state=...) to the proxy's local
/// listener from the native side, bypassing any browser/system proxy on loopback.
#[tauri::command]
fn submit_oauth_callback(url: String) -> Result<(), String> {
    quotio_core::submit_oauth_callback(&url)
}

// ---------------------------------------------------------------------------
// Native OAuth (no proxy dependency)
// ---------------------------------------------------------------------------

#[tauri::command]
fn native_oauth_start(
    provider_id: String,
) -> Result<quotio_core::native_oauth::OAuthStartResponse, String> {
    quotio_core::native_oauth::start_oauth(&provider_id)
}

#[tauri::command]
fn native_oauth_complete(
    login_id: String,
    state: State<'_, DesktopState>,
) -> Result<quotio_core::native_oauth::OAuthCompleteResponse, String> {
    let result = quotio_core::native_oauth::complete_oauth(&login_id)?;
    if result.status == "success" {
        if let Ok(core) = state.core.lock() {
            core.dedup_codex_auth();
        }
    }
    Ok(result)
}

#[tauri::command]
fn native_oauth_cancel(login_id: Option<String>) -> Result<(), String> {
    quotio_core::native_oauth::cancel_oauth(login_id.as_deref())
}

#[tauri::command]
fn native_oauth_submit_callback(login_id: String, callback_url: String) -> Result<(), String> {
    quotio_core::native_oauth::submit_callback_url(&login_id, &callback_url)
}

#[tauri::command]
fn import_auth_token(provider_id: String, content: String) -> Result<(), String> {
    quotio_core::native_oauth::import_auth_token(&provider_id, &content)
}

/// 启动 Kiro 组织(awsidc)/ Builder ID 的 AWS SSO 设备流登录。
#[tauri::command]
fn kiro_idc_start(
    login_option: String,
    start_url: Option<String>,
    region: Option<String>,
) -> Result<quotio_core::kiro_idc::KiroIdcStartResponse, String> {
    quotio_core::kiro_idc::start_login(&login_option, start_url.as_deref(), region.as_deref())
}

/// 轮询一次 Kiro 设备流授权状态;成功时凭据已落盘,前端刷新账号即可。
#[tauri::command]
fn kiro_idc_poll(
    state: State<'_, DesktopState>,
) -> Result<quotio_core::kiro_idc::KiroIdcPollResponse, String> {
    let result = quotio_core::kiro_idc::poll_login()?;
    if result.status == "success" {
        // 新增 kiro 号后热同步 sidecar + 代理配置,让新账号立刻可路由(不重启代理核心)。
        if let Ok(mut core) = state.core.lock() {
            let _ = core.reconcile_kiro_accounts();
        }
    }
    Ok(result)
}

/// 取消进行中的 Kiro 设备流登录。
#[tauri::command]
fn kiro_idc_cancel() -> Result<(), String> {
    quotio_core::kiro_idc::cancel_login();
    Ok(())
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
    _lock_error: &str,
) -> Result<ManagementApiClient, String> {
    let mut core = lock_core(&state.core);
    core.management_client().map_err(|error| error.to_string())
}

fn apply_management_snapshot(
    state: &State<'_, DesktopState>,
    snapshot: ManagementSnapshot,
    _lock_error: &str,
) -> Result<AppState, String> {
    let mut core = lock_core(&state.core);
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
/// Position the menu-bar panel near the tray icon (like macOS NSMenu), then
/// show + focus it.  Falls back to top-right if no tray rect is available.
fn position_menubar(panel: &tauri::WebviewWindow, tray_rect: Option<tauri::Rect>) {
    let panel_w = 360.0_f64;
    if let Some(rect) = tray_rect {
        if let Ok(Some(monitor)) = panel.primary_monitor() {
            let scale = monitor.scale_factor();
            let screen_h = monitor.size().height as f64 / scale;
            let screen_w = monitor.size().width as f64 / scale;
            let panel_h = panel
                .outer_size()
                .map(|s| s.height as f64 / scale)
                .unwrap_or(500.0);
            let icon_pos = rect.position.to_logical::<f64>(scale);
            let icon_size = rect.size.to_logical::<f64>(scale);
            let icon_cx = icon_pos.x + icon_size.width / 2.0;
            let icon_y = icon_pos.y;
            // Center horizontally on icon, clamp to screen
            let x = (icon_cx - panel_w / 2.0).clamp(8.0, screen_w - panel_w - 8.0);
            // Taskbar at bottom → place panel above; taskbar at top → place below
            let y = if icon_y > screen_h / 2.0 {
                (icon_y - panel_h - 8.0).max(8.0)
            } else {
                icon_y + icon_size.height + 8.0
            };
            let _ = panel.set_position(tauri::LogicalPosition::new(x, y));
        }
    } else if let Ok(Some(monitor)) = panel.primary_monitor() {
        let scale = monitor.scale_factor();
        let screen_w = monitor.size().width as f64 / scale;
        let x = (screen_w - panel_w - 12.0).max(0.0);
        let _ = panel.set_position(tauri::LogicalPosition::new(x, 20.0));
    }
    let _ = panel.show();
    let _ = panel.set_focus();
}

/// Show/hide the tray "menu bar" quota panel, anchored near the tray icon.
fn toggle_menubar(app: &AppHandle, tray_rect: Option<tauri::Rect>) {
    let Some(panel) = app.get_webview_window("menubar") else {
        return;
    };
    if panel.is_visible().unwrap_or(false) {
        let _ = panel.hide();
    } else {
        // The blur auto-hide may have just fired; re-show is fine — position_menubar
        // calls show() + set_focus() which will bring the panel back.
        position_menubar(&panel, tray_rect);
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
            // Isolate each iteration: a panic anywhere in the body must NOT kill
            // the only queue consumer (that would silently stop usage collection
            // for the rest of the process). Catch it and continue next tick.
            if let Err(panic) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let prepared = app.try_state::<DesktopState>().and_then(|state| {
                    let mut core = lock_core(&state.core);
                    let client = core.management_client().ok()?;
                    Some((client, core.usage_store()))
                });
                if let Some((client, store)) = prepared {
                    // Re-assert usage telemetry every ~30s so the queue keeps filling
                    // even when nothing else refreshes the management snapshot.
                    if tick % 20 == 0 {
                        let _ = tauri::async_runtime::block_on(
                            client.set_usage_statistics_enabled(true),
                        );
                    }
                    let events = tauri::async_runtime::block_on(client.fetch_usage_events(2000))
                        .unwrap_or_default();
                    if !events.is_empty() {
                        let inserted = store.insert_events(&events);
                        quotio_core::append_request_errors(&events);
                        if inserted > 0 {
                            let _ = app.emit("usage-updated", inserted);
                        }
                        // 智能调度：目标账号刚出现失败请求（往往是 5h 额度耗尽的 429）——
                        // 秒级触发重拉配额重选，不等 5 分钟轮询（期间池子里只有空号）。
                        let recheck = app
                            .try_state::<DesktopState>()
                            .and_then(|state| {
                                state.core.lock().ok().map(|mut core| {
                                    core.scheduler_should_recheck_for_failures(&events)
                                })
                            })
                            .unwrap_or(false);
                        if recheck {
                            refresh_quotas_and_reschedule(&app);
                        }
                    }
                }

                // 智能调度（每 ~30s）：① 当前号 5h 窗口到点刷新（纯内存判断）→ 提前重评估;
                // ② 主动探一次当前目标号的 token——过期/被禁就提前切,把「闲置一段时间后
                // 首个请求」的报错也省掉(只探目标号、不全量,几乎不增上游负担)。
                if tick % 20 == 10 {
                    let (due, targets, proxy_url) = app
                        .try_state::<DesktopState>()
                        .and_then(|state| {
                            state.core.lock().ok().map(|core| {
                                (
                                    core.scheduler_reset_due(),
                                    core.scheduler_active_target_files(),
                                    core.proxy_upstream_url(),
                                )
                            })
                        })
                        .unwrap_or((false, Vec::new(), None));
                    // 网络探测在锁外执行,不阻塞 UI 命令。None(网络抖动)不切,避免误判。
                    let target_unhealthy = targets.iter().any(|(provider_id, file)| {
                        quotio_core::quota::fetch_quota_for_file(
                            provider_id,
                            file,
                            proxy_url.as_deref(),
                        )
                        .map(|quota| quota.is_forbidden || quota.is_auth_failure())
                        .unwrap_or(false)
                    });
                    if due || target_unhealthy {
                        refresh_quotas_and_reschedule(&app);
                    }
                }

                // Codex 一键启动监控（每 ~3s）：用户自己退出 Codex（没点「停止」）时，
                // 自动还原 auth.json/config.toml 并通知前端刷新状态。
                // 进程探测（tasklist）在锁外执行，两次取锁都是纯内存操作，不阻塞 UI 命令；
                // 无会话时 probe 直接返回 None，零开销。
                if tick % 2 == 0 {
                    let probe = app.try_state::<DesktopState>().and_then(|state| {
                        state
                            .core
                            .lock()
                            .ok()
                            .and_then(|core| core.codex_monitor_probe())
                    });
                    if let Some((generation, probe)) = probe {
                        let alive = probe.run();
                        let restored = app
                            .try_state::<DesktopState>()
                            .and_then(|state| {
                                state
                                    .core
                                    .lock()
                                    .ok()
                                    .map(|mut core| core.codex_monitor_apply(generation, alive))
                            })
                            .unwrap_or(false);
                        if restored {
                            let _ = app.emit("codex-launch-changed", false);
                        }
                    }
                }
            })) {
                let msg = panic
                    .downcast_ref::<&str>()
                    .copied()
                    .or_else(|| panic.downcast_ref::<String>().map(|s| s.as_str()))
                    .unwrap_or("unknown");
                eprintln!("[usage-collector] panic caught (tick {tick}): {msg}");
            }
            tick = tick.wrapping_add(1);
            std::thread::sleep(std::time::Duration::from_millis(USAGE_COLLECTOR_POLL_MS));
        }
    });
}

/// 锁外全量拉一次配额 → 存入 + 跑一轮智能调度；池子有变化则通知前端。
/// 给后台触发器用（5h 窗口到点 / 目标账号请求失败），不依赖前端轮询。
fn refresh_quotas_and_reschedule(app: &AppHandle) {
    let proxy_url = app.try_state::<DesktopState>().and_then(|state| {
        state
            .core
            .lock()
            .ok()
            .and_then(|core| core.proxy_upstream_url())
    });
    let quotas = quotio_core::quota::fetch_all_quotas_streaming(proxy_url.as_deref(), &|_| {});
    let changed = app
        .try_state::<DesktopState>()
        .and_then(|state| {
            state.core.lock().ok().map(|mut core| {
                core.store_quotas(quotas);
                core.scheduler_reconcile()
            })
        })
        .unwrap_or(false);
    if changed {
        let _ = app.emit("scheduler-changed", ());
    }
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
        // 自动更新（仅桌面）：updater 负责检查/下载/安装新版本，process 提供
        // 安装后 relaunch 的能力。两者都需要前端 @tauri-apps/plugin-* 配合。
        builder = builder
            .plugin(tauri_plugin_updater::Builder::new().build())
            .plugin(tauri_plugin_process::init());
    }
    builder
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
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
                        rect,
                        ..
                    } = event
                    {
                        toggle_menubar(tray.app_handle(), Some(rect));
                    }
                });
            // Tauri's tray shows nothing without an explicit icon. Reuse the
            // app's embedded window icon so the tray is visible on Windows.
            if let Some(icon) = app.default_window_icon().cloned() {
                tray_builder = tray_builder.icon(icon);
            }
            tray_builder.build(app)?;

            // Menu-bar panel: apply Mica blur. This is a pinned always-on-top quota
            // HUD toggled from the tray icon, so it deliberately does NOT auto-hide
            // on focus loss — clicking another app must not make it vanish. Collapse
            // it by clicking the tray icon again (toggle_menubar) or "打开 Quotio".
            if let Some(panel) = app.get_webview_window("menubar") {
                let _ = panel.set_effects(WindowEffectsConfig {
                    effects: vec![Effect::Mica],
                    ..Default::default()
                });
            }

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
            list_dream_skin_themes,
            import_dream_skin_theme,
            codex_start,
            codex_stop,
            codex_launch_active,
            codex_active_profile,
            codex_repair_session_visibility,
            fetch_codex_models,
            discover_available_models,
            refresh_fallback_route_state,
            credential_status,
            clear_remote_management_key,
            open_config_root,
            open_logs_dir,
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
            export_auth_files,
            list_local_accounts,
            list_custom_providers,
            key_router_available,
            add_custom_provider,
            delete_custom_provider,
            update_custom_provider,
            add_provider_key,
            remove_provider_key,
            toggle_provider_key,
            list_api_keys,
            get_api_key_bindings,
            set_api_key_binding,
            add_api_key,
            remove_api_key,
            update_api_key,
            refresh_quotas,
            consume_codex_reset_credit,
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
            clear_request_logs,
            count_request_logs,
            add_management_api_key,
            update_management_api_key,
            delete_management_api_key,
            delete_management_api_key_by_index,
            set_management_auth_file_disabled,
            reorder_provider_accounts,
            delete_management_auth_file,
            delete_all_management_auth_files,
            start_management_oauth,
            poll_management_oauth,
            submit_oauth_callback,
            native_oauth_start,
            native_oauth_complete,
            native_oauth_cancel,
            native_oauth_submit_callback,
            import_auth_token,
            fetch_codex_reasoning_levels,
            kiro_idc_start,
            kiro_idc_poll,
            kiro_idc_cancel,
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

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use quotio_types::{AgentConfigMode, AgentConfigStorageOption, AgentSetupMode};

    use super::*;

    #[test]
    fn configure_agent_blocking_returns_unknown_agent_error_without_state() {
        let request = AgentConfigurationRequest {
            agent_id: "unknown-agent".to_string(),
            mode: AgentConfigMode::Automatic,
            setup_mode: AgentSetupMode::Proxy,
            storage_option: AgentConfigStorageOption::Json,
            proxy_url: String::new(),
            api_key: String::new(),
            model_slots: BTreeMap::new(),
            use_oauth: false,
            available_models: Vec::new(),
            reasoning_effort: String::new(),
        };

        let error = configure_agent_blocking(request).expect_err("unknown agent should fail");

        assert_eq!(error, "未知 CLI agent。");
    }

    #[test]
    fn configure_agent_blocking_builds_known_manual_configuration() {
        let request = AgentConfigurationRequest {
            agent_id: "gemini-cli".to_string(),
            mode: AgentConfigMode::Manual,
            setup_mode: AgentSetupMode::Proxy,
            storage_option: AgentConfigStorageOption::Shell,
            proxy_url: "http://127.0.0.1:28317".to_string(),
            api_key: "test-key".to_string(),
            model_slots: BTreeMap::new(),
            use_oauth: false,
            available_models: Vec::new(),
            reasoning_effort: String::new(),
        };

        let result = configure_agent_blocking(request).expect("known agent should configure");

        assert!(result.success);
        assert_eq!(result.mode, AgentConfigMode::Manual);
        assert_eq!(result.raw_configs.len(), 1);
        assert!(result
            .shell_config
            .as_deref()
            .is_some_and(|config| config.contains("GEMINI_API_KEY=\"test-key\"")));
        assert!(result.backup_path.is_none());
    }
}
