use serde::Serialize;
use std::path::Path;

#[cfg(target_os = "windows")]
use std::{
    ffi::OsStr,
    fs::{self, File},
    io::{self, Read, Seek, SeekFrom},
    path::PathBuf,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

#[cfg(target_os = "windows")]
use serde::Deserialize;

#[cfg(target_os = "windows")]
const START_TIMEOUT: Duration = Duration::from_secs(110);
#[cfg(target_os = "windows")]
const STOP_TIMEOUT: Duration = Duration::from_secs(45);
#[cfg(target_os = "windows")]
const COMMAND_POLL_INTERVAL: Duration = Duration::from_millis(25);
#[cfg(target_os = "windows")]
const COMMAND_OUTPUT_LIMIT: usize = 2_000;

#[cfg(target_os = "windows")]
const START_RUNTIME_FILES: [&str; 8] = [
    "scripts/start-dream-skin.ps1",
    "scripts/restore-dream-skin.ps1",
    "scripts/common-windows.ps1",
    "scripts/config-utf8.ps1",
    "scripts/injector.mjs",
    "assets/dream-skin.css",
    "assets/renderer-inject.js",
    "assets/dream-reference.png",
];

#[cfg(target_os = "windows")]
const CLEANUP_RUNTIME_FILES: [&str; 3] = [
    "scripts/restore-dream-skin.ps1",
    "scripts/common-windows.ps1",
    "scripts/config-utf8.ps1",
];

pub const DEFAULT_THEME_ID: &str = "dream";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DreamSkinThemeSummary {
    pub id: String,
    pub name: String,
    pub built_in: bool,
}

#[cfg(target_os = "windows")]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DreamSkinThemeManifest {
    schema_version: u32,
    id: String,
    name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    image: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    brand_title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    brand_subtitle: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    signature: Option<String>,
}

#[cfg(target_os = "windows")]
struct ResolvedDreamSkinTheme {
    id: String,
    directory: PathBuf,
}

pub fn normalize_theme_id(value: &str) -> Result<String, String> {
    let value = value.trim();
    let value = if value.is_empty() {
        DEFAULT_THEME_ID
    } else {
        value
    };
    let mut chars = value.chars();
    let first = chars
        .next()
        .ok_or_else(|| "Dream Skin 主题 ID 不能为空".to_string())?;
    if value.len() > 32
        || !first.is_ascii_alphanumeric()
        || !chars.all(|character| {
            character.is_ascii_alphanumeric() || character == '-' || character == '_'
        })
    {
        return Err(format!("无效的 Dream Skin 主题 ID：{value}"));
    }
    Ok(value.to_string())
}

pub fn list_themes() -> Result<Vec<DreamSkinThemeSummary>, String> {
    #[cfg(target_os = "windows")]
    {
        let bundled_root = bundled_theme_root();
        let custom_root = custom_theme_root();
        let mut themes = collect_themes(&bundled_root, true, true)?;
        themes.extend(collect_themes(&custom_root, false, false)?);
        themes.sort_by(|left, right| match (left.built_in, right.built_in) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            (true, true) => bundled_theme_rank(&left.id)
                .cmp(&bundled_theme_rank(&right.id))
                .then_with(|| left.name.cmp(&right.name)),
            (false, false) => left
                .name
                .cmp(&right.name)
                .then_with(|| left.id.cmp(&right.id)),
        });
        return Ok(themes);
    }

    #[cfg(not(target_os = "windows"))]
    Err("Dream Skin 主题库目前仅集成到 Windows 启动方案".to_string())
}

pub fn import_theme(
    image_path: &Path,
    requested_name: Option<&str>,
) -> Result<DreamSkinThemeSummary, String> {
    #[cfg(target_os = "windows")]
    {
        return import_theme_into_root(image_path, requested_name, &custom_theme_root());
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = (image_path, requested_name);
        Err("Dream Skin 自定义主题目前仅集成到 Windows 启动方案".to_string())
    }
}

/// Dream Skin's Windows launcher only supports the signed Store package. A
/// custom/portable Codex path must keep using Quotio's normal launcher.
pub fn validate_codex_target(executable: &Path) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        let normalized = executable
            .to_string_lossy()
            .replace('/', "\\")
            .to_ascii_lowercase();
        if !normalized.contains("\\windowsapps\\") {
            return Err(
                "Dream Skin 目前仅支持 Windows 商店版 Codex；自定义/绿色版路径请关闭该开关"
                    .to_string(),
            );
        }
        return Ok(());
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = executable;
        Err("Dream Skin 启动方案目前仅支持 Windows".to_string())
    }
}

/// Launch the official Store Codex with loopback CDP and wait until the
/// vendored injector reports a verified first render.
pub fn start(theme_id: &str) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        let theme = resolve_theme(theme_id)?;
        validate_runtime_files(&START_RUNTIME_FILES)?;
        let theme_dir = powershell_compatible_path(&theme.directory);
        run_script(
            "start-dream-skin.ps1",
            &[
                "-Theme",
                &theme.id,
                "-ThemeDir",
                &theme_dir,
                "-RestartExisting",
                "-NoFallbackRelaunch",
            ],
            START_TIMEOUT,
            "启用 Dream Skin",
        )?;
        return Ok(());
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = theme_id;
        Err("Dream Skin 启动方案目前仅支持 Windows".to_string())
    }
}

/// Remove a recorded injector/CDP session without reopening Codex. Returns
/// false when no Dream Skin state exists, so normal launch profiles stay fast.
pub fn cleanup_if_present() -> Result<bool, String> {
    #[cfg(target_os = "windows")]
    {
        let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") else {
            return Ok(false);
        };
        let state_path = Path::new(&local_app_data)
            .join("CodexDreamSkin")
            .join("state.json");
        if !state_path.is_file() {
            return Ok(false);
        }

        validate_runtime_files(&CLEANUP_RUNTIME_FILES)?;
        run_script(
            "restore-dream-skin.ps1",
            &["-ForceRestart", "-NoRelaunch"],
            STOP_TIMEOUT,
            "清理 Dream Skin",
        )?;
        return Ok(true);
    }

    #[cfg(not(target_os = "windows"))]
    Ok(false)
}

#[cfg(target_os = "windows")]
fn validate_runtime_files(required: &[&str]) -> Result<(), String> {
    let root = quotio_platform::dream_skin_resource_dir();
    let missing = required
        .iter()
        .map(|relative| root.join(relative))
        .filter(|path| !path.is_file())
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(format!("Dream Skin 运行资源不完整：{}", missing.join("、")))
    }
}

#[cfg(target_os = "windows")]
fn bundled_theme_root() -> PathBuf {
    quotio_platform::dream_skin_resource_dir().join("themes")
}

#[cfg(target_os = "windows")]
fn custom_theme_root() -> PathBuf {
    quotio_platform::app_config_dir()
        .join("dream-skin")
        .join("themes")
}

#[cfg(target_os = "windows")]
fn bundled_theme_rank(theme_id: &str) -> usize {
    match theme_id {
        "dream" => 0,
        "aurora" => 1,
        "midnight" => 2,
        "pink-custom" => 3,
        "wealth-worker" => 4,
        "red-white-scifi" => 5,
        "clear-custom" => 6,
        "inspiration-cosmos" => 7,
        "purple-night" => 8,
        "hatsune-miku" => 9,
        "stage-black-gold" => 10,
        _ => usize::MAX,
    }
}

#[cfg(target_os = "windows")]
fn collect_themes(
    root: &Path,
    built_in: bool,
    required: bool,
) -> Result<Vec<DreamSkinThemeSummary>, String> {
    if !root.is_dir() {
        return if required {
            Err(format!("Dream Skin 主题目录不存在：{}", root.display()))
        } else {
            Ok(Vec::new())
        };
    }

    let entries = fs::read_dir(root)
        .map_err(|error| format!("无法读取 Dream Skin 主题目录 {}：{error}", root.display()))?;
    let mut themes = Vec::new();
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) if !required => {
                eprintln!("忽略无法读取的 Dream Skin 用户主题目录项：{error}");
                continue;
            }
            Err(error) => return Err(format!("无法读取 Dream Skin 内置主题目录项：{error}")),
        };
        let directory = entry.path();
        if !directory.is_dir() {
            continue;
        }
        let Some(directory_id) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        if normalize_theme_id(&directory_id).is_err() {
            continue;
        }
        if !built_in && bundled_theme_root().join(&directory_id).is_dir() {
            continue;
        }
        match validate_theme_package(&directory, &directory_id) {
            Ok(manifest) => themes.push(DreamSkinThemeSummary {
                id: manifest.id,
                name: manifest.name,
                built_in,
            }),
            Err(error) if !required => {
                eprintln!(
                    "忽略损坏的 Dream Skin 用户主题 {}：{error}",
                    directory.display()
                );
            }
            Err(error) => return Err(error),
        }
    }
    Ok(themes)
}

#[cfg(target_os = "windows")]
fn resolve_theme(theme_id: &str) -> Result<ResolvedDreamSkinTheme, String> {
    let id = normalize_theme_id(theme_id)?;
    for root in [bundled_theme_root(), custom_theme_root()] {
        let candidate = root.join(&id);
        if !candidate.is_dir() {
            continue;
        }
        let directory = canonical_theme_directory(&root, &candidate)?;
        validate_theme_package(&directory, &id)?;
        return Ok(ResolvedDreamSkinTheme { id, directory });
    }
    Err(format!("Dream Skin 主题不存在：{id}"))
}

#[cfg(target_os = "windows")]
fn canonical_theme_directory(root: &Path, candidate: &Path) -> Result<PathBuf, String> {
    let canonical_root = fs::canonicalize(root)
        .map_err(|error| format!("无法解析 Dream Skin 主题根目录 {}：{error}", root.display()))?;
    let canonical_candidate = fs::canonicalize(candidate).map_err(|error| {
        format!(
            "无法解析 Dream Skin 主题目录 {}：{error}",
            candidate.display()
        )
    })?;
    if !canonical_candidate.starts_with(&canonical_root) {
        return Err(format!(
            "Dream Skin 主题目录越过了允许的根目录：{}",
            candidate.display()
        ));
    }
    Ok(canonical_candidate)
}

#[cfg(target_os = "windows")]
fn validate_theme_package(
    directory: &Path,
    expected_id: &str,
) -> Result<DreamSkinThemeManifest, String> {
    let manifest_path = directory.join("theme.json");
    let css_path = directory.join("theme.css");
    if !manifest_path.is_file() || !css_path.is_file() {
        return Err(format!(
            "Dream Skin 主题资源不完整：{}",
            directory.display()
        ));
    }
    let manifest_size = fs::metadata(&manifest_path)
        .map_err(|error| format!("无法读取主题配置 {}：{error}", manifest_path.display()))?
        .len();
    if manifest_size == 0 || manifest_size > 64 * 1024 {
        return Err(format!(
            "Dream Skin 主题配置大小异常：{}",
            manifest_path.display()
        ));
    }
    let css_size = fs::metadata(&css_path)
        .map_err(|error| format!("无法读取主题样式 {}：{error}", css_path.display()))?
        .len();
    if css_size > 1024 * 1024 {
        return Err(format!(
            "Dream Skin 主题样式超过 1 MB：{}",
            css_path.display()
        ));
    }
    let manifest_text = fs::read_to_string(&manifest_path)
        .map_err(|error| format!("无法读取主题配置 {}：{error}", manifest_path.display()))?;
    let manifest: DreamSkinThemeManifest = serde_json::from_str(&manifest_text)
        .map_err(|error| format!("主题配置格式错误 {}：{error}", manifest_path.display()))?;
    if manifest.schema_version != 1 {
        return Err(format!(
            "不支持的 Dream Skin 主题版本：{}",
            manifest.schema_version
        ));
    }
    if manifest.id != expected_id || normalize_theme_id(&manifest.id)? != expected_id {
        return Err(format!(
            "Dream Skin 主题 ID 与目录不一致：{} / {expected_id}",
            manifest.id
        ));
    }
    if normalized_theme_name(Some(&manifest.name), Path::new(expected_id)) != manifest.name {
        return Err(format!("Dream Skin 主题名称无效：{}", manifest.name));
    }
    if let Some(image) = manifest.image.as_deref() {
        if Path::new(image).file_name() != Some(OsStr::new(image)) {
            return Err("Dream Skin 主题图片必须位于主题目录内".to_string());
        }
        validate_theme_image_file(&directory.join(image))?;
    }
    Ok(manifest)
}

#[cfg(target_os = "windows")]
fn import_theme_into_root(
    image_path: &Path,
    requested_name: Option<&str>,
    root: &Path,
) -> Result<DreamSkinThemeSummary, String> {
    let source = fs::canonicalize(image_path)
        .map_err(|error| format!("无法读取主题图片 {}：{error}", image_path.display()))?;
    validate_theme_image_file(&source)?;
    let bytes = fs::read(&source)
        .map_err(|error| format!("无法读取 Dream Skin 主题图片 {}：{error}", source.display()))?;
    let extension = normalized_image_extension(&source)?;
    let image_name = format!("background.{extension}");
    let name = normalized_theme_name(requested_name, &source);

    fs::create_dir_all(root).map_err(|error| {
        format!(
            "无法创建 Dream Skin 用户主题目录 {}：{error}",
            root.display()
        )
    })?;
    let id = format!("user-{}", &uuid::Uuid::new_v4().simple().to_string()[..20]);
    let final_directory = root.join(&id);
    let staging_directory = root.join(format!(".{id}.{}.tmp", std::process::id()));
    if final_directory.exists() || staging_directory.exists() {
        return Err("Dream Skin 主题 ID 冲突，请重试".to_string());
    }

    let import_result = (|| -> Result<(), String> {
        fs::create_dir(&staging_directory).map_err(|error| {
            format!(
                "无法创建 Dream Skin 临时主题目录 {}：{error}",
                staging_directory.display()
            )
        })?;
        fs::write(staging_directory.join(&image_name), bytes)
            .map_err(|error| format!("无法保存 Dream Skin 主题图片：{error}"))?;
        fs::write(
            staging_directory.join("theme.css"),
            ":root.codex-dream-skin {\r\n  --dream-theme-name: \"user\";\r\n}\r\n",
        )
        .map_err(|error| format!("无法保存 Dream Skin 主题样式：{error}"))?;
        let manifest = DreamSkinThemeManifest {
            schema_version: 1,
            id: id.clone(),
            name: name.clone(),
            image: Some(image_name),
            brand_title: Some(name.clone()),
            brand_subtitle: Some("CUSTOM DREAM SKIN".to_string()),
            signature: Some(name.clone()),
        };
        let manifest_json = serde_json::to_string_pretty(&manifest)
            .map_err(|error| format!("无法生成 Dream Skin 主题配置：{error}"))?;
        fs::write(
            staging_directory.join("theme.json"),
            format!("{manifest_json}\r\n"),
        )
        .map_err(|error| format!("无法保存 Dream Skin 主题配置：{error}"))?;
        validate_theme_package(&staging_directory, &id)?;
        fs::rename(&staging_directory, &final_directory)
            .map_err(|error| format!("无法完成 Dream Skin 主题导入：{error}"))?;
        Ok(())
    })();
    if let Err(error) = import_result {
        let _ = fs::remove_dir_all(&staging_directory);
        return Err(error);
    }

    Ok(DreamSkinThemeSummary {
        id,
        name,
        built_in: false,
    })
}

#[cfg(target_os = "windows")]
fn normalized_theme_name(requested_name: Option<&str>, source: &Path) -> String {
    let fallback = source
        .file_stem()
        .and_then(OsStr::to_str)
        .unwrap_or("自定义主题");
    let candidate = requested_name
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(fallback);
    let normalized = candidate
        .chars()
        .filter(|character| !character.is_control())
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(80)
        .collect::<String>();
    if normalized.is_empty() {
        "自定义主题".to_string()
    } else {
        normalized
    }
}

#[cfg(target_os = "windows")]
fn normalized_image_extension(path: &Path) -> Result<&'static str, String> {
    match path
        .extension()
        .and_then(OsStr::to_str)
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("png") => Ok("png"),
        Some("jpg" | "jpeg") => Ok("jpg"),
        Some("webp") => Ok("webp"),
        _ => Err("Dream Skin 仅支持 PNG、JPEG、WebP 图片".to_string()),
    }
}

#[cfg(target_os = "windows")]
fn validate_theme_image_file(path: &Path) -> Result<(), String> {
    let extension = normalized_image_extension(path)?;
    let metadata = fs::metadata(path)
        .map_err(|error| format!("无法读取 Dream Skin 主题图片 {}：{error}", path.display()))?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > 16 * 1024 * 1024 {
        return Err("Dream Skin 主题图片必须是 1 字节至 16 MB 的文件".to_string());
    }
    let mut file = File::open(path)
        .map_err(|error| format!("无法读取 Dream Skin 主题图片 {}：{error}", path.display()))?;
    let mut header = [0_u8; 12];
    let header_len = file
        .read(&mut header)
        .map_err(|error| format!("无法读取 Dream Skin 主题图片头 {}：{error}", path.display()))?;
    let header = &header[..header_len];
    let signature_matches = match extension {
        "png" => header.starts_with(b"\x89PNG\r\n\x1a\n"),
        "jpg" => header.starts_with(&[0xff, 0xd8, 0xff]),
        "webp" => header.len() >= 12 && &header[..4] == b"RIFF" && &header[8..12] == b"WEBP",
        _ => false,
    };
    if !signature_matches {
        return Err(format!(
            "Dream Skin 图片内容与扩展名不匹配：{}",
            path.display()
        ));
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn run_script(
    script_name: &str,
    script_args: &[&str],
    timeout: Duration,
    action: &str,
) -> Result<String, String> {
    let script = quotio_platform::dream_skin_resource_dir()
        .join("scripts")
        .join(script_name);
    if !script.is_file() {
        return Err(format!("Dream Skin 脚本不存在：{}", script.display()));
    }

    let mut arguments = vec![
        "-NoProfile".to_string(),
        "-NonInteractive".to_string(),
        "-ExecutionPolicy".to_string(),
        "Bypass".to_string(),
        "-File".to_string(),
        powershell_compatible_path(&script),
    ];
    arguments.extend(script_args.iter().map(|value| (*value).to_string()));
    let borrowed = arguments.iter().map(String::as_str).collect::<Vec<_>>();
    // The script launches long-lived Codex/Node descendants. Capturing PowerShell
    // with anonymous pipes lets those descendants inherit the pipe write handles;
    // after PowerShell exits, wait_with_output() then waits forever for EOF. Use
    // regular temporary files instead: reading a file reaches its current EOF even
    // while a detached descendant still owns an inherited file handle.
    let output = run_command_with_timeout_to_files(Path::new("powershell.exe"), &borrowed, timeout)
        .map_err(|error| format!("{action}失败，无法启动 PowerShell：{error}"))?;

    let details = [output.stderr.trim(), output.stdout.trim()]
        .into_iter()
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>()
        .join("；");
    if output.timed_out {
        return Err(format!(
            "{action}超时{}",
            if details.is_empty() {
                String::new()
            } else {
                format!("：{details}")
            }
        ));
    }
    if output.status_code != Some(0) {
        return Err(format!(
            "{action}失败{}",
            if details.is_empty() {
                String::new()
            } else {
                format!("：{details}")
            }
        ));
    }
    Ok(details)
}

#[cfg(target_os = "windows")]
fn run_command_with_timeout_to_files(
    executable: &Path,
    args: &[&str],
    timeout: Duration,
) -> io::Result<quotio_platform::CommandOutput> {
    let mut stdout_file = tempfile::tempfile()?;
    let mut stderr_file = tempfile::tempfile()?;
    let mut command = Command::new(executable);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout_file.try_clone()?))
        .stderr(Stdio::from(stderr_file.try_clone()?));

    use std::os::windows::process::CommandExt;
    command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW

    let mut child = command.spawn()?;
    let started_at = Instant::now();
    let (status_code, timed_out) = loop {
        if let Some(status) = child.try_wait()? {
            break (status.code(), false);
        }
        if started_at.elapsed() >= timeout {
            let _ = child.kill();
            break (child.wait()?.code(), true);
        }
        thread::sleep(COMMAND_POLL_INTERVAL);
    };

    Ok(quotio_platform::CommandOutput {
        status_code,
        stdout: read_captured_output(&mut stdout_file)?,
        stderr: read_captured_output(&mut stderr_file)?,
        timed_out,
    })
}

#[cfg(target_os = "windows")]
fn read_captured_output(file: &mut File) -> io::Result<String> {
    file.seek(SeekFrom::Start(0))?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    let decoded = String::from_utf8_lossy(&bytes);
    let mut chars = decoded.chars();
    let mut output = chars
        .by_ref()
        .take(COMMAND_OUTPUT_LIMIT)
        .collect::<String>();
    if chars.next().is_some() {
        output.push_str("\n…");
    }
    Ok(output)
}

#[cfg(target_os = "windows")]
fn powershell_compatible_path(path: &Path) -> String {
    let value = path.to_string_lossy();
    if let Some(rest) = value.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{rest}")
    } else if let Some(rest) = value.strip_prefix(r"\\?\") {
        rest.to_string()
    } else {
        value.into_owned()
    }
}

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use super::*;

    #[test]
    fn store_target_is_required_for_dream_skin() {
        assert!(validate_codex_target(Path::new(
            r"C:\Program Files\WindowsApps\OpenAI.Codex_1.2.3.4_x64__test\app\Codex.exe"
        ))
        .is_ok());
        assert!(validate_codex_target(Path::new(r"D:\Apps\Codex\Codex.exe")).is_err());
    }

    #[test]
    fn bundled_runtime_is_resolvable_from_the_workspace() {
        validate_runtime_files(&START_RUNTIME_FILES)
            .expect("Dream Skin runtime resources should resolve in development");
        let themes = collect_themes(&bundled_theme_root(), true, true)
            .expect("bundled themes should resolve");
        let ids = themes
            .into_iter()
            .map(|theme| theme.id)
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(ids.len(), 11);
        for theme_id in [
            "dream",
            "aurora",
            "midnight",
            "pink-custom",
            "wealth-worker",
            "red-white-scifi",
            "clear-custom",
            "inspiration-cosmos",
            "purple-night",
            "hatsune-miku",
            "stage-black-gold",
        ] {
            assert!(ids.contains(theme_id), "missing bundled theme {theme_id}");
        }
    }

    #[test]
    fn powershell_path_drops_windows_extended_prefix() {
        assert_eq!(
            powershell_compatible_path(Path::new(r"\\?\D:\Quotio\start.ps1")),
            r"D:\Quotio\start.ps1"
        );
        assert_eq!(
            powershell_compatible_path(Path::new(r"\\?\UNC\server\share\start.ps1")),
            r"\\server\share\start.ps1"
        );
    }

    #[test]
    fn theme_ids_are_path_safe() {
        assert_eq!(normalize_theme_id(""), Ok(DEFAULT_THEME_ID.to_string()));
        assert_eq!(normalize_theme_id("aurora"), Ok("aurora".to_string()));
        assert_eq!(
            normalize_theme_id("user-123_test"),
            Ok("user-123_test".to_string())
        );
        assert!(normalize_theme_id("..\\custom").is_err());
    }

    #[test]
    fn image_import_creates_a_reusable_user_theme() {
        use base64::Engine as _;

        let temporary = tempfile::tempdir().unwrap();
        let image_path = temporary.path().join("夏日星空.png");
        let image = base64::engine::general_purpose::STANDARD
            .decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAusB9Y9ZQmcAAAAASUVORK5CYII=")
            .unwrap();
        fs::write(&image_path, image).unwrap();
        let theme_root = temporary.path().join("themes");

        let imported = import_theme_into_root(&image_path, None, &theme_root).unwrap();

        assert!(imported.id.starts_with("user-"));
        assert_eq!(imported.name, "夏日星空");
        assert!(!imported.built_in);
        let theme_directory = theme_root.join(&imported.id);
        let manifest = validate_theme_package(&theme_directory, &imported.id).unwrap();
        assert_eq!(manifest.name, "夏日星空");
        assert_eq!(manifest.image.as_deref(), Some("background.png"));
        let listed = collect_themes(&theme_root, false, false).unwrap();
        assert_eq!(listed, vec![imported]);
    }

    #[test]
    fn image_import_rejects_a_fake_extension() {
        let temporary = tempfile::tempdir().unwrap();
        let image_path = temporary.path().join("fake.png");
        fs::write(&image_path, b"not an image").unwrap();
        assert!(
            import_theme_into_root(&image_path, None, &temporary.path().join("themes"))
                .unwrap_err()
                .contains("扩展名不匹配")
        );
    }

    #[test]
    fn detached_child_does_not_hold_command_completion_open() {
        use std::io::Write;

        let mut script = tempfile::Builder::new().suffix(".ps1").tempfile().unwrap();
        writeln!(
            script,
            r#"
Start-Process -FilePath powershell.exe -ArgumentList @(
  '-NoProfile',
  '-NonInteractive',
  '-Command',
  'Start-Sleep -Seconds 6'
) -WindowStyle Hidden | Out-Null
Write-Output 'parent-done'
"#
        )
        .unwrap();

        script.flush().unwrap();
        let script_path = script.into_temp_path();
        let script_path_text = script_path.to_string_lossy().into_owned();
        let started_at = Instant::now();
        let output = run_command_with_timeout_to_files(
            Path::new("powershell.exe"),
            &[
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
                &script_path_text,
            ],
            Duration::from_secs(2),
        )
        .unwrap();

        assert!(!output.timed_out);
        assert_eq!(
            output.status_code,
            Some(0),
            "stdout={} stderr={}",
            output.stdout,
            output.stderr
        );
        assert!(output.stdout.contains("parent-done"));
        assert!(
            started_at.elapsed() < Duration::from_secs(3),
            "detached child kept the command runner blocked"
        );
    }
}
