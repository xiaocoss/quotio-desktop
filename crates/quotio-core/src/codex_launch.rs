//! Codex 一键启动：探测 Codex 桌面应用、把选中账号凭证注入 `~/.codex/auth.json`、启动 App 或 CLI。
//!
//! 「绑定账号」只为让 Codex 桌面应用能登录启动；实际请求走 Quotio 代理
//! （`~/.codex/config.toml` 的 cliproxyapi provider），绑定的号本身不参与额度调用。

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{json, Value};

const LAUNCH_BACKUP_FILE: &str = "quotio-launch-backup.json";
const BOUND_LOGIN_ONLY_FIELD: &str = "quotio_bound_login_only";
const PREVIOUS_DISABLED_FIELD: &str = "quotio_previous_disabled";

/// Windows：让子进程不弹出黑色控制台窗口（用于 powershell/taskkill 这类后台调用）。
#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// 新建一个不弹控制台窗口的 Command（仅 Windows 加标志；其它平台原样）。
fn quiet_command(program: &str) -> Command {
    let mut command = Command::new(program);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
}

// ---------- 路径 ----------

fn codex_home() -> PathBuf {
    quotio_platform::expand_home_path("~/.codex")
}

fn codex_auth_path() -> PathBuf {
    codex_home().join("auth.json")
}

fn codex_config_path() -> PathBuf {
    codex_home().join("config.toml")
}

fn codex_launch_backup_path() -> PathBuf {
    codex_home().join(LAUNCH_BACKUP_FILE)
}

fn proxy_auth_dir() -> PathBuf {
    quotio_platform::expand_home_path("~/.cli-proxy-api")
}

fn proxy_account_path_in(dir: &Path, key: &str) -> PathBuf {
    dir.join(format!("{key}.json"))
}

// ---------- App 探测 ----------

/// 从给定的 WindowsApps 风格根目录里找 `<pkg>/app/Codex.exe`（pkg 名含 "codex"），
/// 返回版本号最高的那个。纯函数，便于测试。
pub fn detect_codex_app_path_in(roots: &[PathBuf]) -> Option<PathBuf> {
    let mut best: Option<(Vec<u64>, PathBuf)> = None;
    for root in roots {
        let Ok(entries) = std::fs::read_dir(root) else {
            continue;
        };
        for entry in entries.flatten() {
            if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_lowercase();
            if !name.contains("codex") {
                continue;
            }
            let candidate = entry.path().join("app").join("Codex.exe");
            if !candidate.exists() {
                continue;
            }
            let version = parse_version_from_dir_name(&name);
            let better = match &best {
                None => true,
                Some((best_version, _)) => version > *best_version,
            };
            if better {
                best = Some((version, candidate));
            }
        }
    }
    best.map(|(_, path)| path)
}

/// 从包目录名里抽第一段点分数字版本
/// （如 `openai.codex_26.608.1337.0_x64__hash` → `[26, 608, 1337, 0]`）。
fn parse_version_from_dir_name(name: &str) -> Vec<u64> {
    for token in name.split(|c| c == '_' || c == ' ') {
        let nums: Vec<u64> = token
            .split('.')
            .filter_map(|part| part.parse::<u64>().ok())
            .collect();
        if nums.len() >= 2 {
            return nums;
        }
    }
    Vec::new()
}

/// Windows：用 `Get-AppxPackage` 拿 Codex 安装目录
/// （比直接扫 WindowsApps 更可靠，后者子目录常被 ACL 挡）。
#[cfg(target_os = "windows")]
fn detect_codex_via_appx() -> Option<PathBuf> {
    let output = quiet_command("powershell")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "(Get-AppxPackage -Name *Codex* | Select-Object -First 1).InstallLocation",
        ])
        .output()
        .ok()?;
    let location = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if location.is_empty() {
        return None;
    }
    let exe = PathBuf::from(location).join("app").join("Codex.exe");
    exe.exists().then_some(exe)
}

/// 探测 Codex 桌面应用可执行文件。
/// Windows：先 Appx 再扫盘；macOS：`/Applications/Codex.app`；其它：None。
pub fn detect_codex_app_path() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        if let Some(path) = detect_codex_via_appx() {
            return Some(path);
        }
        let mut roots = Vec::new();
        for drive in b'A'..=b'Z' {
            let drive = drive as char;
            let root = if drive == 'C' {
                format!(r"{drive}:\Program Files\WindowsApps")
            } else {
                format!(r"{drive}:\WindowsApps")
            };
            let path = PathBuf::from(root);
            if path.exists() {
                roots.push(path);
            }
        }
        return detect_codex_app_path_in(&roots);
    }
    #[cfg(target_os = "macos")]
    {
        let path = PathBuf::from("/Applications/Codex.app/Contents/MacOS/Codex");
        return path.exists().then_some(path);
    }
    #[allow(unreachable_code)]
    None
}

/// 带缓存的探测：整个进程只真正跑一次（Appx/扫盘较慢），供 agent 检测等高频调用，
/// 避免每次刷新都跑一遍 powershell。需要强制重探时用 [`detect_codex_app_path`]。
pub fn detect_codex_app_path_cached() -> Option<PathBuf> {
    use std::sync::OnceLock;
    static CACHE: OnceLock<Option<PathBuf>> = OnceLock::new();
    CACHE.get_or_init(detect_codex_app_path).clone()
}

// ---------- 从代理拉取真实模型 ----------

/// 从运行中的代理拉它实际服务的 codex/gpt-5 模型（OpenAI 兼容 `GET /v1/models`）。
/// best-effort：拿不到（代理没跑 / 无 key / 网络错）就返回空，前端回退到内置列表。
pub fn fetch_proxy_codex_models(endpoint: &str, api_key: &str) -> Vec<String> {
    let base = endpoint.trim_end_matches('/').trim_end_matches("/v1");
    if base.is_empty() || api_key.is_empty() {
        return Vec::new();
    }
    let url = format!("{base}/v1/models");
    let agent = ureq::builder()
        .timeout_connect(std::time::Duration::from_secs(4))
        .timeout_read(std::time::Duration::from_secs(6))
        .build();
    let response = match agent
        .get(&url)
        .set("Authorization", &format!("Bearer {api_key}"))
        .set("Accept", "application/json")
        .call()
    {
        Ok(response) => response,
        Err(_) => return Vec::new(),
    };
    let json: Value = match response.into_json() {
        Ok(json) => json,
        Err(_) => return Vec::new(),
    };
    json.get("data")
        .and_then(|data| data.as_array())
        .map(|models| {
            models
                .iter()
                .filter_map(|model| model.get("id").and_then(|id| id.as_str()))
                .filter(|id| {
                    let lower = id.to_lowercase();
                    (lower.contains("gpt-5") || lower.contains("codex")) && !lower.contains("image")
                })
                .map(|id| id.to_string())
                .collect()
        })
        .unwrap_or_default()
}

// ---------- 账号列表 + 绑定注入 ----------

/// 一个可绑定的 Codex 账号（来自 `~/.cli-proxy-api` 的 codex auth 文件）。
#[derive(Debug, Clone, serde::Serialize)]
pub struct CodexAccountRef {
    /// 文件名去掉 `.json`，作为稳定 key。
    pub key: String,
    pub email: String,
    pub disabled: bool,
}

fn is_codex_auth(file_name: &str, value: &Value) -> bool {
    let name = file_name.to_lowercase();
    let type_codex = value.get("type").and_then(|v| v.as_str()) == Some("codex");
    let has_tokens = value.get("access_token").is_some()
        && value.get("id_token").is_some()
        && value.get("refresh_token").is_some();
    (name.contains("codex") || type_codex) && has_tokens
}

/// 列出可绑定的 Codex 账号（供前端下拉）。
pub fn list_codex_accounts() -> Vec<CodexAccountRef> {
    let dir = proxy_auth_dir();
    let mut accounts = Vec::new();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return accounts;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let file_name = entry.file_name().to_string_lossy().to_string();
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        if !is_codex_auth(&file_name, &value) {
            continue;
        }
        let key = file_name.trim_end_matches(".json").to_string();
        let email = value
            .get("email")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let disabled = value
            .get("disabled")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        accounts.push(CodexAccountRef {
            key,
            email,
            disabled,
        });
    }
    accounts.sort_by(|a, b| a.email.cmp(&b.email));
    accounts
}

/// 把 cli-proxy 的 codex auth（扁平）转成 Codex 官方 auth.json（嵌套）。纯函数，便于测试。
pub fn build_codex_auth_json(src: &Value) -> Value {
    let get = |key: &str| {
        src.get(key)
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string()
    };
    json!({
        "auth_mode": "chatgpt",
        "OPENAI_API_KEY": Value::Null,
        "tokens": {
            "id_token": get("id_token"),
            "access_token": get("access_token"),
            "refresh_token": get("refresh_token"),
            "account_id": get("account_id"),
        },
        "last_refresh": src.get("last_refresh").cloned().unwrap_or(Value::Null),
    })
}

/// 读取某个绑定账号的源文件。
fn read_proxy_codex_account(key: &str) -> Result<Value, String> {
    let path = proxy_auth_dir().join(format!("{key}.json"));
    let text = std::fs::read_to_string(&path)
        .map_err(|e| format!("读取账号文件失败 {}: {e}", path.display()))?;
    serde_json::from_str::<Value>(&text).map_err(|e| format!("解析账号文件失败: {e}"))
}

fn read_proxy_account_from(path: &Path) -> Result<Value, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("读取账号文件失败 {}: {e}", path.display()))?;
    serde_json::from_str::<Value>(&text).map_err(|e| format!("解析账号文件失败: {e}"))
}

fn write_proxy_account_to(path: &Path, value: &Value) -> Result<(), String> {
    let text =
        serde_json::to_string_pretty(value).map_err(|e| format!("序列化账号文件失败: {e}"))?;
    std::fs::write(path, text).map_err(|e| format!("写入账号文件失败 {}: {e}", path.display()))
}

/// 切换 Codex 绑定账号：释放旧绑定账号，锁定新绑定账号。
/// 锁定方式是写入 `disabled=true`，让 CLIProxyAPI 不把它放进反代池。
pub fn apply_bound_account_selection(previous_key: &str, next_key: &str) -> Result<(), String> {
    apply_bound_account_selection_in(&proxy_auth_dir(), previous_key, next_key)
}

fn apply_bound_account_selection_in(
    dir: &Path,
    previous_key: &str,
    next_key: &str,
) -> Result<(), String> {
    let previous_key = previous_key.trim();
    let next_key = next_key.trim();

    if !previous_key.is_empty() && previous_key != next_key {
        release_bound_account_login_only_in(dir, previous_key)?;
    }
    if !next_key.is_empty() {
        mark_bound_account_login_only_in(dir, next_key)?;
    }
    Ok(())
}

/// 把账号标记为“仅用于 Codex 登录”，并保留它原来的 disabled 状态，供切号时恢复。
pub fn mark_bound_account_login_only(key: &str) -> Result<(), String> {
    mark_bound_account_login_only_in(&proxy_auth_dir(), key)
}

fn mark_bound_account_login_only_in(dir: &Path, key: &str) -> Result<(), String> {
    let path = proxy_account_path_in(dir, key.trim());
    let mut value = read_proxy_account_from(&path)?;
    let object = value
        .as_object_mut()
        .ok_or_else(|| format!("账号文件不是 JSON 对象: {}", path.display()))?;
    let was_already_bound = object
        .get(BOUND_LOGIN_ONLY_FIELD)
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    let previous_disabled = if was_already_bound {
        object
            .get(PREVIOUS_DISABLED_FIELD)
            .and_then(|value| value.as_bool())
            .unwrap_or_else(|| {
                object
                    .get("disabled")
                    .and_then(|value| value.as_bool())
                    .unwrap_or(false)
            })
    } else {
        object
            .get("disabled")
            .and_then(|value| value.as_bool())
            .unwrap_or(false)
    };
    object.insert("disabled".to_string(), Value::Bool(true));
    object.insert(BOUND_LOGIN_ONLY_FIELD.to_string(), Value::Bool(true));
    object.insert(
        PREVIOUS_DISABLED_FIELD.to_string(),
        Value::Bool(previous_disabled),
    );
    write_proxy_account_to(&path, &value)
}

/// 释放由 Quotio 绑定逻辑禁用的账号，恢复为绑定前的 disabled 状态。
pub fn release_bound_account_login_only(key: &str) -> Result<(), String> {
    release_bound_account_login_only_in(&proxy_auth_dir(), key)
}

fn release_bound_account_login_only_in(dir: &Path, key: &str) -> Result<(), String> {
    let key = key.trim();
    if key.is_empty() {
        return Ok(());
    }
    let path = proxy_account_path_in(dir, key);
    if !path.exists() {
        return Ok(());
    }
    let mut value = read_proxy_account_from(&path)?;
    let object = value
        .as_object_mut()
        .ok_or_else(|| format!("账号文件不是 JSON 对象: {}", path.display()))?;
    let is_bound_login_only = object
        .get(BOUND_LOGIN_ONLY_FIELD)
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    if !is_bound_login_only {
        return Ok(());
    }
    let previous_disabled = object
        .get(PREVIOUS_DISABLED_FIELD)
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    object.insert("disabled".to_string(), Value::Bool(previous_disabled));
    object.remove(BOUND_LOGIN_ONLY_FIELD);
    object.remove(PREVIOUS_DISABLED_FIELD);
    write_proxy_account_to(&path, &value)
}

/// 绑定账号：把选中号的凭证写进 `~/.codex/auth.json`。
pub fn inject_bound_account(key: &str) -> Result<(), String> {
    let src = read_proxy_codex_account(key)?;
    let auth = build_codex_auth_json(&src);
    let home = codex_home();
    std::fs::create_dir_all(&home).map_err(|e| format!("创建 ~/.codex 失败: {e}"))?;
    let auth_path = codex_auth_path();
    let text =
        serde_json::to_string_pretty(&auth).map_err(|e| format!("序列化 auth.json 失败: {e}"))?;
    std::fs::write(&auth_path, text).map_err(|e| format!("写入 auth.json 失败: {e}"))?;
    Ok(())
}

// ---------- 启动生命周期：备份 / 还原 / 杀进程 ----------

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct CodexLaunchBackup {
    /// 启动前 `~/.codex/auth.json` 内容（None = 当时不存在）。
    auth_json: Option<String>,
    /// 启动前 `~/.codex/config.toml` 内容（None = 当时不存在）。
    config_toml: Option<String>,
}

/// 一次「启动」的会话：记录启动的进程 pid，用于「停止」或关闭软件时杀进程。
/// 文件恢复通过 `~/.codex/quotio-launch-backup.json` 完成。
pub struct CodexSession {
    pub pid: Option<u32>,
    pub launch_mode: String,
}

/// 读当前 auth.json + config.toml 内容（None = 文件不存在），用于启动前备份。
pub fn read_codex_state() -> (Option<String>, Option<String>) {
    read_codex_state_in(&codex_home())
}

fn read_codex_state_in(home: &Path) -> (Option<String>, Option<String>) {
    (
        std::fs::read_to_string(home.join("auth.json")).ok(),
        std::fs::read_to_string(home.join("config.toml")).ok(),
    )
}

/// 把启动前的 auth.json + config.toml 状态写进一个固定备份文件。
pub fn write_launch_backup() -> Result<(), String> {
    write_launch_backup_in(&codex_home())
}

fn write_launch_backup_in(home: &Path) -> Result<(), String> {
    std::fs::create_dir_all(home).map_err(|e| format!("创建 ~/.codex 失败: {e}"))?;
    let backup_path = home.join(LAUNCH_BACKUP_FILE);
    if backup_path.exists() {
        return Err(format!("Codex 启动备份已存在: {}", backup_path.display()));
    }
    let (auth_json, config_toml) = read_codex_state_in(home);
    let backup = CodexLaunchBackup {
        auth_json,
        config_toml,
    };
    let text = serde_json::to_string_pretty(&backup)
        .map_err(|e| format!("序列化 Codex 启动备份失败: {e}"))?;
    std::fs::write(&backup_path, text).map_err(|e| format!("写入 Codex 启动备份失败: {e}"))
}

pub fn launch_backup_exists() -> bool {
    codex_launch_backup_path().exists()
}

/// 从固定备份文件恢复 auth.json + config.toml，恢复成功后删除备份文件。
pub fn restore_codex_state_from_launch_backup() -> Result<(), String> {
    restore_codex_state_from_launch_backup_in(&codex_home())
}

fn restore_codex_state_from_launch_backup_in(home: &Path) -> Result<(), String> {
    let backup_path = home.join(LAUNCH_BACKUP_FILE);
    let text = std::fs::read_to_string(&backup_path)
        .map_err(|e| format!("读取 Codex 启动备份失败 {}: {e}", backup_path.display()))?;
    let backup: CodexLaunchBackup =
        serde_json::from_str(&text).map_err(|e| format!("解析 Codex 启动备份失败: {e}"))?;
    restore_one(&home.join("auth.json"), &backup.auth_json)?;
    restore_one(&home.join("config.toml"), &backup.config_toml)?;
    std::fs::remove_file(&backup_path)
        .map_err(|e| format!("删除 Codex 启动备份失败 {}: {e}", backup_path.display()))?;
    Ok(())
}

/// 把 auth.json + config.toml 还原到备份内容（None = 原本不存在 → 删除）。
pub fn restore_codex_state(auth: &Option<String>, config: &Option<String>) -> Result<(), String> {
    restore_one(&codex_auth_path(), auth)?;
    restore_one(&codex_config_path(), config)?;
    Ok(())
}

fn restore_one(path: &Path, backup: &Option<String>) -> Result<(), String> {
    match backup {
        Some(content) => {
            std::fs::write(path, content).map_err(|e| format!("还原 {} 失败: {e}", path.display()))
        }
        None => {
            if path.exists() {
                let _ = std::fs::remove_file(path);
            }
            Ok(())
        }
    }
}

/// 杀掉进程（Windows 用 taskkill /T 连子进程一起）。best-effort。
pub fn kill_process(pid: u32) {
    #[cfg(target_os = "windows")]
    {
        let _ = quiet_command("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .output();
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = Command::new("kill").args(["-9", &pid.to_string()]).output();
    }
}

/// 关掉所有 Codex 桌面应用进程（按名字，best-effort，不弹窗）。
/// 启动前调用：避免运行中的实例把我们写的 config.toml 覆盖掉，并让它重启时读到新配置。
pub fn close_codex_app() {
    #[cfg(target_os = "windows")]
    {
        let _ = quiet_command("taskkill")
            .args(["/IM", "Codex.exe", "/T", "/F"])
            .output();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = Command::new("pkill").args(["-f", "Codex.app"]).output();
    }
    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    {
        let _ = Command::new("pkill").args(["-x", "codex"]).output();
    }
}

// ---------- 启动 ----------

/// App 模式：直接 spawn `Codex.exe`，独立于 Quotio 进程。返回 pid。
///
/// 商店版（MSIX）Codex 装在 `WindowsApps` 下，普通进程直接 CreateProcess 会被
/// ACL 拒绝（os error 5），此时回退到 shell 启动（PowerShell `Start-Process` →
/// `shell:AppsFolder` 应用入口激活）；shell 启动拿不到子进程句柄，
/// 启动后按进程名探测 pid（启动前已 `close_codex_app`，找到的就是新实例），
/// 探测不到则返回 None（停止时仍有按名 taskkill 兜底）。
pub fn launch_codex_app(exe: &Path) -> Result<Option<u32>, String> {
    if !exe.exists() {
        return Err(format!("Codex 应用不存在: {}", exe.display()));
    }
    let mut command = Command::new(exe);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        command.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
    }
    match command.spawn() {
        Ok(child) => Ok(Some(child.id())),
        #[cfg(target_os = "windows")]
        Err(err)
            if err.kind() == std::io::ErrorKind::PermissionDenied && is_windowsapps_path(exe) =>
        {
            launch_codex_app_via_shell(exe)?;
            Ok(resolve_codex_app_pid_within(std::time::Duration::from_secs(
                8,
            )))
        }
        Err(err) => Err(format!("启动 Codex 应用失败: {err}")),
    }
}

/// 路径是否在 WindowsApps（商店应用安装目录）下。
#[cfg(target_os = "windows")]
fn is_windowsapps_path(path: &Path) -> bool {
    path.to_string_lossy()
        .to_lowercase()
        .contains("\\windowsapps\\")
}

/// PowerShell 单引号字符串转义（`'` → `''`）。
#[cfg(target_os = "windows")]
fn escape_powershell_single_quoted(value: &str) -> String {
    value.replace('\'', "''")
}

#[cfg(target_os = "windows")]
fn run_powershell(script: &str) -> Result<std::process::Output, String> {
    quiet_command("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .output()
        .map_err(|e| format!("调用 PowerShell 失败: {e}"))
}

#[cfg(target_os = "windows")]
fn run_powershell_expect_success(script: &str) -> Result<std::process::Output, String> {
    let output = run_powershell(script)?;
    if output.status.success() {
        return Ok(output);
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stderr_head: String = stderr.trim().chars().take(300).collect();
    Err(if stderr_head.is_empty() {
        format!("PowerShell 退出码 {}", output.status)
    } else {
        stderr_head
    })
}

/// WindowsApps 直接 spawn 被拒后的 shell 启动：
/// 先 `Start-Process` exe 路径（走 ShellExecute），再退到商店应用入口激活。
#[cfg(target_os = "windows")]
fn launch_codex_app_via_shell(exe: &Path) -> Result<(), String> {
    let script = format!(
        "Start-Process -FilePath '{}' -ErrorAction Stop | Out-Null",
        escape_powershell_single_quoted(&exe.to_string_lossy())
    );
    let direct_error = match run_powershell_expect_success(&script) {
        Ok(_) => return Ok(()),
        Err(error) => error,
    };
    let app_id = detect_codex_store_app_id().ok_or_else(|| {
        format!("启动 Codex 应用失败（商店版需 shell 启动）: {direct_error}；且未检测到商店应用入口")
    })?;
    let script = format!(
        "Start-Process -FilePath ('shell:AppsFolder\\' + '{}') -ErrorAction Stop | Out-Null",
        escape_powershell_single_quoted(&app_id)
    );
    run_powershell_expect_success(&script)
        .map(|_| ())
        .map_err(|error| format!("启动 Codex 应用失败（商店入口 {app_id}）: {error}"))
}

/// 探测商店版 Codex 的 AppUserModelId（`<PackageFamilyName>!<AppId>`）：
/// 先 `Get-StartApps`（开始菜单注册的真实入口），拿不到再用 Appx 包名拼 `!App`。
#[cfg(target_os = "windows")]
fn detect_codex_store_app_id() -> Option<String> {
    let stdout_first_line = |output: std::process::Output| {
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(str::trim)
            .find(|line| !line.is_empty())
            .map(str::to_string)
    };
    if let Some(app_id) = run_powershell(
        "$entry = Get-StartApps | Where-Object { $_.AppID -like 'OpenAI.Codex_*' } | Select-Object -First 1; if ($entry) { [string]$entry.AppID }",
    )
    .ok()
    .and_then(stdout_first_line)
    {
        return Some(app_id);
    }
    run_powershell(
        "$pkg = Get-AppxPackage -Name *Codex* -ErrorAction SilentlyContinue | Sort-Object -Property Version -Descending | Select-Object -First 1; if ($pkg) { [string]($pkg.PackageFamilyName + '!App') }",
    )
    .ok()
    .and_then(stdout_first_line)
}

/// 在超时时间内按进程名轮询 Codex 应用 pid（shell 启动拿不到子进程句柄时用）。
#[cfg(target_os = "windows")]
fn resolve_codex_app_pid_within(timeout: std::time::Duration) -> Option<u32> {
    let started = std::time::Instant::now();
    loop {
        if let Ok(output) = run_powershell(
            "(Get-Process -Name Codex -ErrorAction SilentlyContinue | Select-Object -First 1).Id",
        ) {
            if let Ok(pid) = String::from_utf8_lossy(&output.stdout).trim().parse::<u32>() {
                return Some(pid);
            }
        }
        if started.elapsed() >= timeout {
            return None;
        }
        std::thread::sleep(std::time::Duration::from_millis(300));
    }
}

/// 构建 CLI 启动命令（用默认 `~/.codex`，配置已写好，直接跑 `codex`）。纯函数，便于测试。
pub fn build_cli_launch_command() -> String {
    "codex".to_string()
}

/// CLI 模式：开一个终端跑 `codex`（用默认 `~/.codex`，配置已写好）。返回终端进程 pid（用于停止时杀掉）。
pub fn launch_codex_cli() -> Result<Option<u32>, String> {
    let codex_cmd = build_cli_launch_command();
    #[cfg(target_os = "windows")]
    {
        if let Ok(child) = Command::new("wt")
            .args(["powershell", "-NoExit", "-Command", &codex_cmd])
            .spawn()
        {
            return Ok(Some(child.id()));
        }
        Command::new("cmd")
            .args([
                "/c",
                "start",
                "powershell",
                "-NoExit",
                "-Command",
                &codex_cmd,
            ])
            .spawn()
            .map_err(|e| format!("打开终端失败: {e}"))?;
        Ok(None)
    }
    #[cfg(target_os = "macos")]
    {
        let script = format!("tell application \"Terminal\" to do script \"{codex_cmd}\"");
        Command::new("osascript")
            .args(["-e", &script])
            .spawn()
            .map_err(|e| format!("打开终端失败: {e}"))?;
        Ok(None)
    }
    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    {
        for term in ["x-terminal-emulator", "gnome-terminal", "konsole", "xterm"] {
            if let Ok(child) = Command::new(term).args(["-e", &codex_cmd]).spawn() {
                return Ok(Some(child.id()));
            }
        }
        Err("未找到可用终端".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_codex_exe_under_windowsapps() {
        let tmp = std::env::temp_dir().join("ql_codex_detect_test_a");
        let _ = std::fs::remove_dir_all(&tmp);
        let app = tmp.join("OpenAI.Codex_26.608.1337.0_x64__hash").join("app");
        std::fs::create_dir_all(&app).unwrap();
        std::fs::write(app.join("Codex.exe"), b"x").unwrap();
        let found = detect_codex_app_path_in(&[tmp.clone()]);
        assert_eq!(found, Some(app.join("Codex.exe")));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn detect_picks_highest_version() {
        let tmp = std::env::temp_dir().join("ql_codex_detect_test_b");
        let _ = std::fs::remove_dir_all(&tmp);
        for dir in [
            "OpenAI.Codex_1.2.0_x64__h",
            "OpenAI.Codex_26.608.1337.0_x64__h",
        ] {
            let app = tmp.join(dir).join("app");
            std::fs::create_dir_all(&app).unwrap();
            std::fs::write(app.join("Codex.exe"), b"x").unwrap();
        }
        let found = detect_codex_app_path_in(&[tmp.clone()]).unwrap();
        assert!(found.to_string_lossy().contains("26.608.1337.0"));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn builds_codex_auth_json_from_proxy_account() {
        let src = json!({
            "access_token": "AT",
            "id_token": "IT",
            "refresh_token": "RT",
            "account_id": "ACC",
            "last_refresh": "2026-06-10T11:04:53+08:00",
            "email": "x@example.com",
            "type": "codex",
            "disabled": true,
        });
        let auth = build_codex_auth_json(&src);
        assert_eq!(auth["auth_mode"], "chatgpt");
        assert!(auth["OPENAI_API_KEY"].is_null());
        assert_eq!(auth["tokens"]["id_token"], "IT");
        assert_eq!(auth["tokens"]["access_token"], "AT");
        assert_eq!(auth["tokens"]["refresh_token"], "RT");
        assert_eq!(auth["tokens"]["account_id"], "ACC");
        assert_eq!(auth["last_refresh"], "2026-06-10T11:04:53+08:00");
    }

    #[test]
    fn cli_launch_command_runs_codex() {
        assert!(build_cli_launch_command().contains("codex"));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn recognizes_windowsapps_paths_case_insensitively() {
        assert!(is_windowsapps_path(Path::new(
            r"C:\Program Files\WindowsApps\OpenAI.Codex_26.609.3341.0_x64__2p2nqsd0c76g0\app\Codex.exe"
        )));
        assert!(is_windowsapps_path(Path::new(r"D:\windowsapps\pkg\app\Codex.exe")));
        assert!(!is_windowsapps_path(Path::new(
            r"C:\Users\me\AppData\Local\Programs\Codex\Codex.exe"
        )));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn escapes_powershell_single_quotes() {
        assert_eq!(escape_powershell_single_quoted("plain"), "plain");
        assert_eq!(escape_powershell_single_quoted("it's"), "it''s");
    }

    #[test]
    fn is_codex_auth_detects_by_type_and_tokens() {
        let value = json!({
            "access_token": "a", "id_token": "b", "refresh_token": "c", "type": "codex"
        });
        assert!(is_codex_auth("whatever.json", &value));
        let gemini = json!({ "access_token": "a", "type": "gemini" });
        assert!(!is_codex_auth("gemini-x.json", &gemini));
    }

    #[test]
    fn launch_backup_restores_existing_auth_and_config_files() {
        let home = temp_codex_home("ql_codex_launch_backup_existing");
        std::fs::create_dir_all(&home).unwrap();
        let auth_path = home.join("auth.json");
        let config_path = home.join("config.toml");
        std::fs::write(&auth_path, "original-auth").unwrap();
        std::fs::write(&config_path, "original-config").unwrap();

        write_launch_backup_in(&home).unwrap();
        std::fs::write(&auth_path, "quotio-auth").unwrap();
        std::fs::write(&config_path, "quotio-config").unwrap();

        restore_codex_state_from_launch_backup_in(&home).unwrap();

        assert_eq!(
            std::fs::read_to_string(&auth_path).unwrap(),
            "original-auth"
        );
        assert_eq!(
            std::fs::read_to_string(&config_path).unwrap(),
            "original-config"
        );
        assert!(!home.join("quotio-launch-backup.json").exists());
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn launch_backup_removes_auth_and_config_that_did_not_exist_before_launch() {
        let home = temp_codex_home("ql_codex_launch_backup_missing");
        std::fs::create_dir_all(&home).unwrap();
        let auth_path = home.join("auth.json");
        let config_path = home.join("config.toml");

        write_launch_backup_in(&home).unwrap();
        std::fs::write(&auth_path, "quotio-auth").unwrap();
        std::fs::write(&config_path, "quotio-config").unwrap();

        restore_codex_state_from_launch_backup_in(&home).unwrap();

        assert!(!auth_path.exists());
        assert!(!config_path.exists());
        assert!(!home.join("quotio-launch-backup.json").exists());
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn launch_backup_does_not_overwrite_existing_restore_point() {
        let home = temp_codex_home("ql_codex_launch_backup_no_overwrite");
        std::fs::create_dir_all(&home).unwrap();
        let auth_path = home.join("auth.json");
        let config_path = home.join("config.toml");
        std::fs::write(&auth_path, "original-auth").unwrap();
        std::fs::write(&config_path, "original-config").unwrap();
        write_launch_backup_in(&home).unwrap();

        std::fs::write(&auth_path, "quotio-auth").unwrap();
        std::fs::write(&config_path, "quotio-config").unwrap();

        let error = write_launch_backup_in(&home).expect_err("existing restore point is preserved");
        assert!(error.contains("Codex 启动备份已存在"));

        restore_codex_state_from_launch_backup_in(&home).unwrap();
        assert_eq!(
            std::fs::read_to_string(&auth_path).unwrap(),
            "original-auth"
        );
        assert_eq!(
            std::fs::read_to_string(&config_path).unwrap(),
            "original-config"
        );
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn marking_bound_account_disables_it_and_records_previous_state() {
        let dir = temp_codex_home("ql_codex_bound_mark");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("codex-a.json"),
            r#"{"type":"codex","email":"a@example.com","disabled":false,"access_token":"a","id_token":"i","refresh_token":"r"}"#,
        )
        .unwrap();

        mark_bound_account_login_only_in(&dir, "codex-a").unwrap();

        let value = read_json_for_test(&dir.join("codex-a.json"));
        assert_eq!(value["disabled"], true);
        assert_eq!(value["quotio_bound_login_only"], true);
        assert_eq!(value["quotio_previous_disabled"], false);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn releasing_bound_account_restores_previous_disabled_state() {
        let dir = temp_codex_home("ql_codex_bound_release");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("codex-a.json"),
            r#"{"type":"codex","email":"a@example.com","disabled":false,"access_token":"a","id_token":"i","refresh_token":"r"}"#,
        )
        .unwrap();
        mark_bound_account_login_only_in(&dir, "codex-a").unwrap();

        release_bound_account_login_only_in(&dir, "codex-a").unwrap();

        let value = read_json_for_test(&dir.join("codex-a.json"));
        assert_eq!(value["disabled"], false);
        assert!(value.get("quotio_bound_login_only").is_none());
        assert!(value.get("quotio_previous_disabled").is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn releasing_account_that_was_previously_disabled_keeps_it_disabled() {
        let dir = temp_codex_home("ql_codex_bound_release_disabled");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("codex-a.json"),
            r#"{"type":"codex","email":"a@example.com","disabled":true,"access_token":"a","id_token":"i","refresh_token":"r"}"#,
        )
        .unwrap();
        mark_bound_account_login_only_in(&dir, "codex-a").unwrap();

        release_bound_account_login_only_in(&dir, "codex-a").unwrap();

        let value = read_json_for_test(&dir.join("codex-a.json"));
        assert_eq!(value["disabled"], true);
        assert!(value.get("quotio_bound_login_only").is_none());
        assert!(value.get("quotio_previous_disabled").is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn changing_bound_account_releases_old_and_marks_new() {
        let dir = temp_codex_home("ql_codex_bound_change");
        std::fs::create_dir_all(&dir).unwrap();
        for key in ["codex-a", "codex-b"] {
            std::fs::write(
                dir.join(format!("{key}.json")),
                format!(
                    r#"{{"type":"codex","email":"{key}@example.com","disabled":false,"access_token":"a","id_token":"i","refresh_token":"r"}}"#
                ),
            )
            .unwrap();
        }
        mark_bound_account_login_only_in(&dir, "codex-a").unwrap();

        apply_bound_account_selection_in(&dir, "codex-a", "codex-b").unwrap();

        let old = read_json_for_test(&dir.join("codex-a.json"));
        let new = read_json_for_test(&dir.join("codex-b.json"));
        assert_eq!(old["disabled"], false);
        assert!(old.get("quotio_bound_login_only").is_none());
        assert_eq!(new["disabled"], true);
        assert_eq!(new["quotio_bound_login_only"], true);
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn read_json_for_test(path: &Path) -> Value {
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    fn temp_codex_home(prefix: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("{prefix}_{}_{}", std::process::id(), nanos))
    }
}
