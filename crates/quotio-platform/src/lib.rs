use std::{
    collections::BTreeSet,
    ffi::OsString,
    fs,
    io::{self, Write as _},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{OnceLock, RwLock},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use keyring::{Entry, Error as KeyringError};
use quotio_types::{CredentialAvailability, PlatformFeatureState, PlatformInfo};

const CREDENTIAL_SERVICE: &str = "quotio";
const VERSION_OUTPUT_LIMIT: usize = 2_000;
const COMMAND_POLL_INTERVAL: Duration = Duration::from_millis(25);

static PROXY_RESOURCE_ROOT: OnceLock<RwLock<Option<PathBuf>>> = OnceLock::new();

pub const LOCAL_MANAGEMENT_KEY_ACCOUNT: &str = "local-management-key";
pub const REMOTE_MANAGEMENT_KEY_ACCOUNT: &str = "remote-management-key";
pub const LOCAL_API_KEY_ACCOUNT: &str = "local-api-key";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocatedExecutable {
    pub name: String,
    pub path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandOutput {
    pub status_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub timed_out: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackupFile {
    pub path: PathBuf,
    pub timestamp_unix_seconds: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SafeWriteResult {
    pub path: PathBuf,
    pub backup_path: Option<PathBuf>,
}

pub fn platform_info() -> PlatformInfo {
    PlatformInfo {
        os: std::env::consts::OS.to_string(),
        family: std::env::consts::FAMILY.to_string(),
        arch: std::env::consts::ARCH.to_string(),
    }
}

pub fn app_config_dir() -> PathBuf {
    platform_config_root().join("Quotio")
}

pub fn app_cache_dir() -> PathBuf {
    platform_cache_root().join("Quotio")
}

pub fn app_logs_dir() -> PathBuf {
    if cfg!(target_os = "macos") {
        home_dir().join("Library").join("Logs").join("Quotio")
    } else {
        app_cache_dir().join("logs")
    }
}

pub fn set_proxy_resource_root(path: PathBuf) {
    let root = PROXY_RESOURCE_ROOT.get_or_init(|| RwLock::new(None));
    if let Ok(mut guard) = root.write() {
        *guard = Some(path);
    }
}

pub fn configured_proxy_resource_root() -> Option<PathBuf> {
    PROXY_RESOURCE_ROOT
        .get()
        .and_then(|root| root.read().ok().and_then(|guard| guard.clone()))
}

pub fn proxy_resource_dir() -> PathBuf {
    if let Some(path) = std::env::var_os("QUOTIO_PROXY_RESOURCE_DIR") {
        return PathBuf::from(path);
    }

    let platform = current_proxy_platform();

    if let Some(root) = configured_proxy_resource_root() {
        return root.join(platform);
    }

    let relative = PathBuf::from("resources").join("proxy").join(platform);

    if let Ok(current_dir) = std::env::current_dir() {
        for base in current_dir.ancestors() {
            let candidate = base.join(&relative);
            if candidate.exists() {
                return candidate;
            }
        }
    }

    if let Ok(current_exe) = std::env::current_exe() {
        if let Some(exe_dir) = current_exe.parent() {
            for base in exe_dir.ancestors().take(4) {
                let candidate = base.join(&relative);
                if candidate.exists() {
                    return candidate;
                }
            }
        }
    }

    std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(|parent| parent.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."))
        .join(relative)
}

/// Bundled-resource dir for the kiro-rs sidecar binary. Kept SEPARATE from
/// [`proxy_resource_dir`] on purpose: the proxy-core resolver treats any `.exe`
/// in its resource dir as the core and would otherwise copy kiro-rs over
/// CLIProxyAPI. Derived as a `resources/kiro/<platform>` sibling of the proxy
/// resource root so it reuses the Tauri-configured root without extra setup.
pub fn kiro_resource_dir() -> PathBuf {
    if let Some(path) = std::env::var_os("QUOTIO_KIRO_RESOURCE_DIR") {
        return PathBuf::from(path);
    }

    let platform = current_proxy_platform();

    if let Some(root) = configured_proxy_resource_root() {
        // root = <Resource>/resources/proxy → sibling <Resource>/resources/kiro.
        let base = root.parent().map(|parent| parent.to_path_buf()).unwrap_or(root);
        return base.join("kiro").join(platform);
    }

    let relative = PathBuf::from("resources").join("kiro").join(platform);

    if let Ok(current_dir) = std::env::current_dir() {
        for base in current_dir.ancestors() {
            let candidate = base.join(&relative);
            if candidate.exists() {
                return candidate;
            }
        }
    }

    if let Ok(current_exe) = std::env::current_exe() {
        if let Some(exe_dir) = current_exe.parent() {
            for base in exe_dir.ancestors().take(4) {
                let candidate = base.join(&relative);
                if candidate.exists() {
                    return candidate;
                }
            }
        }
    }

    std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(|parent| parent.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."))
        .join(relative)
}

pub fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
}

pub fn proxy_auth_dir() -> PathBuf {
    home_dir().join(".cli-proxy-api")
}

pub fn current_proxy_platform() -> &'static str {
    if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "darwin"
    } else {
        "linux"
    }
}

pub fn expand_home_path(value: &str) -> PathBuf {
    let trimmed = value.trim();
    if trimmed == "~" {
        return home_dir();
    }

    if let Some(rest) = trimmed.strip_prefix("~/") {
        return home_dir().join(rest);
    }

    if let Some(rest) = trimmed.strip_prefix("~\\") {
        return home_dir().join(rest);
    }

    PathBuf::from(trimmed)
}

pub fn cli_search_dirs(extra_dirs: &[PathBuf]) -> Vec<PathBuf> {
    let mut seen = BTreeSet::new();
    let mut dirs = Vec::new();

    for dir in std::env::var_os("PATH")
        .map(|value| std::env::split_paths(&value).collect::<Vec<_>>())
        .unwrap_or_default()
    {
        push_unique_dir(&mut dirs, &mut seen, dir);
    }

    for dir in extra_dirs {
        push_unique_dir(&mut dirs, &mut seen, dir.clone());
    }

    let home = home_dir();
    for dir in [
        home.join(".cargo").join("bin"),
        home.join(".bun").join("bin"),
        home.join(".deno").join("bin"),
        home.join(".volta").join("bin"),
        home.join(".local").join("bin"),
        home.join(".npm-global").join("bin"),
        home.join(".asdf").join("shims"),
        home.join(".mise").join("shims"),
    ] {
        push_unique_dir(&mut dirs, &mut seen, dir);
    }

    if cfg!(target_os = "windows") {
        for value in [
            std::env::var_os("APPDATA").map(|path| PathBuf::from(path).join("npm")),
            std::env::var_os("LOCALAPPDATA").map(|path| PathBuf::from(path).join("Programs")),
            std::env::var_os("LOCALAPPDATA")
                .map(|path| PathBuf::from(path).join("mise").join("shims")),
            std::env::var_os("ProgramFiles").map(|path| PathBuf::from(path).join("nodejs")),
            std::env::var_os("ProgramFiles(x86)").map(|path| PathBuf::from(path).join("nodejs")),
            Some(home.join("scoop").join("shims")),
            Some(
                home.join("AppData")
                    .join("Local")
                    .join("Microsoft")
                    .join("WinGet")
                    .join("Packages"),
            ),
        ]
        .into_iter()
        .flatten()
        {
            push_unique_dir(&mut dirs, &mut seen, value);
        }
    }

    for dir in node_version_manager_bin_dirs(&home) {
        push_unique_dir(&mut dirs, &mut seen, dir);
    }

    dirs
}

pub fn find_executable(name: &str, extra_dirs: &[PathBuf]) -> Option<PathBuf> {
    let direct = expand_home_path(name);
    if direct.components().count() > 1 && is_executable(&direct) {
        return Some(direct);
    }

    for dir in cli_search_dirs(extra_dirs) {
        for filename in executable_filenames(name) {
            let candidate = dir.join(filename);
            if is_executable(&candidate) {
                return Some(candidate);
            }
        }
    }

    None
}

pub fn find_first_executable(
    names: &[String],
    extra_dirs: &[PathBuf],
) -> Option<LocatedExecutable> {
    names.iter().find_map(|name| {
        find_executable(name, extra_dirs).map(|path| LocatedExecutable {
            name: name.clone(),
            path,
        })
    })
}

pub fn is_executable(path: &Path) -> bool {
    let Ok(metadata) = fs::metadata(path) else {
        return false;
    };

    if !metadata.is_file() {
        return false;
    }

    if cfg!(target_os = "windows") {
        let extension = path
            .extension()
            .and_then(|value| value.to_str())
            .map(|value| value.to_ascii_lowercase());
        return matches!(extension.as_deref(), Some("exe" | "cmd" | "bat" | "ps1"))
            || extension.is_none();
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }

    #[cfg(not(unix))]
    {
        true
    }
}

pub fn run_command_with_timeout(
    executable: &Path,
    args: &[&str],
    timeout: Duration,
) -> io::Result<CommandOutput> {
    let mut command = Command::new(executable);
    command
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW: no console flash
    }
    let mut child = command.spawn()?;

    let started_at = Instant::now();
    loop {
        if child.try_wait()?.is_some() {
            let output = child.wait_with_output()?;
            return Ok(command_output(output, false));
        }

        if started_at.elapsed() >= timeout {
            let _ = child.kill();
            let output = child.wait_with_output()?;
            return Ok(command_output(output, true));
        }

        thread::sleep(COMMAND_POLL_INTERVAL);
    }
}

pub fn read_version(executable: &Path) -> Option<String> {
    for args in [&["--version"][..], &["version"][..], &["-v"][..]] {
        let output = run_command_with_timeout(executable, args, Duration::from_secs(2)).ok()?;
        if output.timed_out {
            continue;
        }

        let text = [output.stdout, output.stderr]
            .into_iter()
            .map(|value| value.trim().to_string())
            .find(|value| !value.is_empty());

        if let Some(text) = text {
            return text.lines().next().map(|value| value.trim().to_string());
        }
    }

    None
}

pub fn write_text_file(
    path: &Path,
    contents: &str,
    sensitive: bool,
    backup_namespace: &str,
) -> io::Result<SafeWriteResult> {
    let backup_path = backup_file(path, backup_namespace)?;
    atomic_write(path, contents.as_bytes(), sensitive)?;
    Ok(SafeWriteResult {
        path: path.to_path_buf(),
        backup_path,
    })
}

pub fn atomic_write(path: &Path, contents: &[u8], sensitive: bool) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("quotio-file");
    let temp_path = parent.join(format!(
        ".{}.quotio-tmp-{}",
        file_name,
        current_unix_millis()
    ));

    {
        let mut file = fs::File::create(&temp_path)?;
        file.write_all(contents)?;
        file.sync_all()?;
    }

    if sensitive {
        set_sensitive_permissions(&temp_path)?;
    }

    if cfg!(target_os = "windows") && path.exists() {
        fs::remove_file(path)?;
    }

    fs::rename(&temp_path, path)?;

    if sensitive {
        set_sensitive_permissions(path)?;
    }

    Ok(())
}

pub fn backup_file(path: &Path, namespace: &str) -> io::Result<Option<PathBuf>> {
    if !path.exists() {
        return Ok(None);
    }

    let backup_dir = backup_dir(namespace);
    fs::create_dir_all(&backup_dir)?;

    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("config");
    let backup_path = backup_dir.join(format!(
        "{}.{}.bak",
        sanitize_filename(file_name),
        current_unix_millis()
    ));
    fs::copy(path, &backup_path)?;
    // 每个源文件只保留最新一份备份：每次写配置/每次「恢复」都会新增一份，
    // 不清理会无限堆积，这里写完新备份就把同文件的旧备份删掉。
    prune_old_backups(&backup_dir, file_name, &backup_path);
    Ok(Some(backup_path))
}

/// 删除 `dir` 里同一源文件的其它 `.bak`（保留 `keep`）。best-effort，删不掉不报错。
fn prune_old_backups(dir: &Path, source_file_name: &str, keep: &Path) {
    let prefix = format!("{}.", sanitize_filename(source_file_name));
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path == keep || !path.is_file() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        if name.starts_with(&prefix) && name.ends_with(".bak") {
            let _ = fs::remove_file(&path);
        }
    }
}

pub fn list_backups(namespace: &str) -> io::Result<Vec<BackupFile>> {
    let dir = backup_dir(namespace);
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut backups = fs::read_dir(dir)?
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let metadata = entry.metadata().ok()?;
            if !metadata.is_file() {
                return None;
            }
            let timestamp_unix_seconds = metadata
                .modified()
                .ok()
                .and_then(system_time_to_unix_seconds)
                .unwrap_or_default();
            Some(BackupFile {
                path: entry.path(),
                timestamp_unix_seconds,
            })
        })
        .collect::<Vec<_>>();

    backups.sort_by(|left, right| {
        right
            .timestamp_unix_seconds
            .cmp(&left.timestamp_unix_seconds)
            .then_with(|| right.path.cmp(&left.path))
    });
    Ok(backups)
}

pub fn restore_backup(
    backup_path: &Path,
    target_path: &Path,
    sensitive: bool,
    backup_namespace: &str,
) -> io::Result<SafeWriteResult> {
    let contents = fs::read(backup_path)?;
    let backup_path = backup_file(target_path, backup_namespace)?;
    atomic_write(target_path, &contents, sensitive)?;
    Ok(SafeWriteResult {
        path: target_path.to_path_buf(),
        backup_path,
    })
}

pub fn credential_availability() -> CredentialAvailability {
    let probe_account = "credential-probe";
    let probe_value = format!("probe-{}", current_unix_millis());

    match set_credential(probe_account, &probe_value).and_then(|_| get_credential(probe_account)) {
        Ok(Some(value)) if value == probe_value => {
            let _ = delete_credential(probe_account);
            CredentialAvailability::Available
        }
        Ok(_) => CredentialAvailability::Unavailable,
        Err(_) => CredentialAvailability::Unavailable,
    }
}

pub fn credential_status_message(availability: &CredentialAvailability) -> String {
    match availability {
        CredentialAvailability::Available => "平台凭据存储可用。".to_string(),
        CredentialAvailability::Unavailable => {
            "平台凭据存储不可用，敏感配置无法安全保存。".to_string()
        }
        CredentialAvailability::Unknown => "尚未检查平台凭据存储。".to_string(),
    }
}

pub fn set_credential(account: &str, value: &str) -> Result<(), String> {
    credential_entry(account)?
        .set_password(value)
        .map_err(keyring_error)
}

pub fn get_credential(account: &str) -> Result<Option<String>, String> {
    match credential_entry(account)?.get_password() {
        Ok(value) => Ok(Some(value)),
        Err(KeyringError::NoEntry) => Ok(None),
        Err(error) => Err(keyring_error(error)),
    }
}

pub fn delete_credential(account: &str) -> Result<(), String> {
    match credential_entry(account)?.delete_credential() {
        Ok(()) => Ok(()),
        Err(KeyringError::NoEntry) => Ok(()),
        Err(error) => Err(keyring_error(error)),
    }
}

pub fn open_file_manager(path: &Path) -> io::Result<()> {
    let target = if path.is_dir() {
        path.to_path_buf()
    } else {
        path.parent().unwrap_or(path).to_path_buf()
    };

    let mut command = if cfg!(target_os = "windows") {
        let mut command = Command::new("explorer");
        command.arg(target);
        command
    } else if cfg!(target_os = "macos") {
        let mut command = Command::new("open");
        command.arg(target);
        command
    } else {
        let mut command = Command::new("xdg-open");
        command.arg(target);
        command
    };

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }

    command.spawn()?.wait()?;
    Ok(())
}

pub fn platform_feature_state(
    launch_at_login_enabled: bool,
    notifications_enabled: bool,
) -> PlatformFeatureState {
    PlatformFeatureState {
        launch_at_login_available: true,
        launch_at_login_enabled,
        notifications_available: true,
        notifications_enabled,
        file_manager_available: true,
        message: "平台能力已接入系统适配层。".to_string(),
    }
}

fn platform_config_root() -> PathBuf {
    if cfg!(target_os = "windows") {
        std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir)
    } else if cfg!(target_os = "macos") {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .map(|home| home.join("Library").join("Application Support"))
            .unwrap_or_else(std::env::temp_dir)
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
            .unwrap_or_else(std::env::temp_dir)
    }
}

fn platform_cache_root() -> PathBuf {
    if cfg!(target_os = "windows") {
        std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir)
    } else if cfg!(target_os = "macos") {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .map(|home| home.join("Library").join("Caches"))
            .unwrap_or_else(std::env::temp_dir)
    } else {
        std::env::var_os("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))
            .unwrap_or_else(std::env::temp_dir)
    }
}

fn push_unique_dir(dirs: &mut Vec<PathBuf>, seen: &mut BTreeSet<OsString>, dir: PathBuf) {
    if dir.as_os_str().is_empty() {
        return;
    }

    let key = if cfg!(target_os = "windows") {
        OsString::from(dir.to_string_lossy().to_ascii_lowercase())
    } else {
        dir.as_os_str().to_os_string()
    };

    if seen.insert(key) {
        dirs.push(dir);
    }
}

fn node_version_manager_bin_dirs(home: &Path) -> Vec<PathBuf> {
    let mut dirs = Vec::new();

    collect_versioned_child_bin_dirs(
        &home.join(".nvm").join("versions").join("node"),
        "bin",
        &mut dirs,
    );

    let xdg_data_home = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".local").join("share"));
    collect_versioned_child_bin_dirs(
        &xdg_data_home.join("fnm").join("node-versions"),
        "installation/bin",
        &mut dirs,
    );
    collect_versioned_child_bin_dirs(
        &home.join(".fnm").join("node-versions"),
        "installation/bin",
        &mut dirs,
    );

    dirs
}

fn collect_versioned_child_bin_dirs(base: &Path, bin_suffix: &str, dirs: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(base) else {
        return;
    };

    let mut children = entries
        .filter_map(Result::ok)
        .filter(|entry| entry.path().is_dir())
        .map(|entry| entry.path())
        .collect::<Vec<_>>();
    children.sort_by(|left, right| right.cmp(left));

    for child in children {
        let mut dir = child;
        for segment in bin_suffix.split('/') {
            dir = dir.join(segment);
        }
        dirs.push(dir);
    }
}

fn executable_filenames(name: &str) -> Vec<String> {
    if !cfg!(target_os = "windows") || Path::new(name).extension().is_some() {
        return vec![name.to_string()];
    }

    ["", ".exe", ".cmd", ".bat", ".ps1"]
        .into_iter()
        .map(|extension| format!("{}{}", name, extension))
        .collect()
}

fn command_output(output: std::process::Output, timed_out: bool) -> CommandOutput {
    CommandOutput {
        status_code: output.status.code(),
        stdout: truncate_text(
            &String::from_utf8_lossy(&output.stdout),
            VERSION_OUTPUT_LIMIT,
        ),
        stderr: truncate_text(
            &String::from_utf8_lossy(&output.stderr),
            VERSION_OUTPUT_LIMIT,
        ),
        timed_out,
    }
}

fn backup_dir(namespace: &str) -> PathBuf {
    app_config_dir()
        .join("backups")
        .join(sanitize_filename(namespace))
}

fn sanitize_filename(value: &str) -> String {
    let sanitized = value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.') {
                character
            } else {
                '_'
            }
        })
        .collect::<String>();

    if sanitized.is_empty() {
        "quotio".to_string()
    } else {
        sanitized
    }
}

fn current_unix_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}

pub fn current_unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}

fn system_time_to_unix_seconds(value: SystemTime) -> Option<u64> {
    value
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|value| value.as_secs())
}

fn truncate_text(value: &str, limit: usize) -> String {
    let mut truncated = value.chars().take(limit).collect::<String>();
    if value.chars().count() > limit {
        truncated.push_str("…");
    }
    truncated
}

pub fn set_sensitive_permissions(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }

    #[cfg(not(unix))]
    {
        let _ = path;
    }

    Ok(())
}

fn credential_entry(account: &str) -> Result<Entry, String> {
    Entry::new(CREDENTIAL_SERVICE, account).map_err(keyring_error)
}

fn keyring_error(error: KeyringError) -> String {
    format!("{}", error)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prune_keeps_only_latest_backup_for_same_source_file() {
        let dir = std::env::temp_dir().join(format!(
            "ql_backup_prune_{}_{}",
            std::process::id(),
            current_unix_millis()
        ));
        fs::create_dir_all(&dir).unwrap();
        let write = |name: &str| fs::write(dir.join(name), b"x").unwrap();
        write("config.toml.100.bak");
        write("config.toml.200.bak");
        write("config.toml.300.bak");
        // 其它源文件的备份不受影响。
        write("settings.json.100.bak");

        let keep = dir.join("config.toml.300.bak");
        prune_old_backups(&dir, "config.toml", &keep);

        assert!(keep.exists());
        assert!(!dir.join("config.toml.100.bak").exists());
        assert!(!dir.join("config.toml.200.bak").exists());
        assert!(dir.join("settings.json.100.bak").exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn backup_then_restore_leaves_a_single_backup() {
        let root = std::env::temp_dir().join(format!(
            "ql_backup_single_{}_{}",
            std::process::id(),
            current_unix_millis()
        ));
        let dir = root.join("backups");
        fs::create_dir_all(&dir).unwrap();
        let source = root.join("config.toml");

        // 模拟多次「写配置前备份」：每轮结束后同源文件只剩一份。
        for round in 0..3 {
            fs::write(&source, format!("content-{round}")).unwrap();
            let file_name = "config.toml";
            let backup_path = dir.join(format!(
                "{}.{}.bak",
                sanitize_filename(file_name),
                current_unix_millis() + round // 避免同毫秒同名
            ));
            fs::copy(&source, &backup_path).unwrap();
            prune_old_backups(&dir, file_name, &backup_path);

            let count = fs::read_dir(&dir)
                .unwrap()
                .filter_map(Result::ok)
                .filter(|entry| {
                    entry
                        .file_name()
                        .to_string_lossy()
                        .starts_with("config.toml.")
                })
                .count();
            assert_eq!(count, 1, "round {round} should keep exactly one backup");
        }
        let _ = fs::remove_dir_all(&root);
    }
}
