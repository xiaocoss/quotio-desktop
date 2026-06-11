//! Codex 一键启动：探测 Codex 桌面应用、把选中账号凭证注入 `~/.codex/auth.json`、启动 App 或 CLI。
//!
//! 「绑定账号」只为让 Codex 桌面应用能登录启动；实际请求走 Quotio 代理
//! （`~/.codex/config.toml` 的 cliproxyapi provider），绑定的号本身不参与额度调用。

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{json, Value};

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

fn proxy_auth_dir() -> PathBuf {
    quotio_platform::expand_home_path("~/.cli-proxy-api")
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

/// 绑定账号：把选中号的凭证写进 `~/.codex/auth.json`（写前备份）。
pub fn inject_bound_account(key: &str) -> Result<(), String> {
    let src = read_proxy_codex_account(key)?;
    let auth = build_codex_auth_json(&src);
    let home = codex_home();
    std::fs::create_dir_all(&home).map_err(|e| format!("创建 ~/.codex 失败: {e}"))?;
    let auth_path = codex_auth_path();
    if auth_path.exists() {
        let backup = home.join("auth.json.quotio.bak");
        let _ = std::fs::copy(&auth_path, &backup);
    }
    let text =
        serde_json::to_string_pretty(&auth).map_err(|e| format!("序列化 auth.json 失败: {e}"))?;
    std::fs::write(&auth_path, text).map_err(|e| format!("写入 auth.json 失败: {e}"))?;
    Ok(())
}

// ---------- 启动生命周期：备份 / 还原 / 杀进程 ----------

/// 一次「启动」的会话：记录启动的进程 pid + 启动前 auth.json/config.toml 的原始内容，
/// 用于「停止」或关闭软件时还原成启动前的样子。
pub struct CodexSession {
    pub pid: Option<u32>,
    /// 启动前 `~/.codex/auth.json` 内容（None = 当时不存在）。
    pub auth_backup: Option<String>,
    /// 启动前 `~/.codex/config.toml` 内容（None = 当时不存在）。
    pub config_backup: Option<String>,
    pub launch_mode: String,
}

/// 读当前 auth.json + config.toml 内容（None = 文件不存在），用于启动前备份。
pub fn read_codex_state() -> (Option<String>, Option<String>) {
    (
        std::fs::read_to_string(codex_auth_path()).ok(),
        std::fs::read_to_string(codex_config_path()).ok(),
    )
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
pub fn launch_codex_app(exe: &Path) -> Result<u32, String> {
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
    let child = command
        .spawn()
        .map_err(|e| format!("启动 Codex 应用失败: {e}"))?;
    Ok(child.id())
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
                "/c", "start", "powershell", "-NoExit", "-Command", &codex_cmd,
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
        let app = tmp
            .join("OpenAI.Codex_26.608.1337.0_x64__hash")
            .join("app");
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
        for dir in ["OpenAI.Codex_1.2.0_x64__h", "OpenAI.Codex_26.608.1337.0_x64__h"] {
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

    #[test]
    fn is_codex_auth_detects_by_type_and_tokens() {
        let value = json!({
            "access_token": "a", "id_token": "b", "refresh_token": "c", "type": "codex"
        });
        assert!(is_codex_auth("whatever.json", &value));
        let gemini = json!({ "access_token": "a", "type": "gemini" });
        assert!(!is_codex_auth("gemini-x.json", &gemini));
    }
}
