//! 从本机 Codex CLI 二进制里提取它内置的「模型目录」(model catalog),落到
//! `~/.codex/quotio-model-catalog.json`,再由 [`crate::agent_config`] 在 Codex 的
//! `config.toml` 里用顶层键 `model_catalog_json` 指过去。
//!
//! **为什么需要**:Codex 的推理档位(reasoning effort)不是固定的,而是按 model slug 从
//! 模型目录里查出来的(`ModelInfo.supported_reasoning_levels`)。官方登录时目录由服务端下发;
//! 而 Quotio 的启动方案把 `model_provider` 换成了自定义的 `cliproxyapi`,Codex 就拿不到目录,
//! 推理档位退回通用默认的 4 档(low/medium/high/xhigh)—— 用户失去 `max` / `ultra`,
//! 滑块配色也跟着变。Codex 提供 `model_catalog_json`(值是 JSON 文件路径)让调用方自带目录。
//!
//! **为什么不打包一份静态快照**:`model_catalog_json` 极可能是**替换**内置目录(Codex 的错误串
//! "model_catalog_json path `…` must contain at least one model" 暗示它就是目录本身)。若打包的
//! 快照比用户的 Codex 旧,新模型会被这份旧目录盖没 —— 把「少两档」换成「看不见新模型」,得不偿失。
//! **从用户自己的 codex 二进制提取**则天然与其版本一致(替换 = 恒等,零风险),且 Codex 一升级、
//! 二进制指纹一变就自动重新提取。**提取失败就不写这个键**,行为与从前完全一致,绝不弄丢用户的模型。

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;
use std::fs;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tempfile::NamedTempFile;

/// 目录 JSON 的落盘位置(Codex 的家目录里,和 config.toml 同级)。
fn catalog_path() -> PathBuf {
    quotio_platform::expand_home_path("~/.codex/quotio-model-catalog.json")
}

/// 记录目录来源和当时完整的候选指纹快照,用来判断候选优先级是否发生变化。
fn meta_path() -> PathBuf {
    quotio_platform::expand_home_path("~/.codex/quotio-model-catalog.meta.json")
}

/// 目录里必定出现、且不随模型增减而变的锚点键。
const ANCHOR: &[u8] = b"\"supported_reasoning_levels\"";
/// 锚点往前找 `"models"` 的最大回溯距离(单个模型条目的前半段不会比这更长)。
const BACK_WINDOW: usize = 16 * 1024;
/// 从目录起始位置往后取多大一块来做花括号配平。目录实测约 300KB,留足余量。
const FORWARD_WINDOW: usize = 8 * 1024 * 1024;
/// 同一进程里只允许一个线程扫描 / 更新目录,避免相互覆盖临时文件和元数据。
static CATALOG_LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Deserialize, Serialize)]
struct CatalogMetadata {
    /// 实际成功提取目录的候选指纹。
    source_fingerprint: String,
    /// 本轮按优先级排列的全部可读候选指纹。
    candidate_fingerprints: Vec<String>,
}

/// 确保目录文件存在且与当前 codex 二进制同步,返回它的路径。
/// 任何一步失败都返回 `None` —— 调用方据此**跳过** `model_catalog_json`,保持旧行为。
pub fn ensure_catalog() -> Option<PathBuf> {
    ensure_catalog_result().ok()
}

/// `ensure_catalog` 的可诊断版本。Task 3 会把这里的错误异步暴露给 UI。
pub fn ensure_catalog_result() -> Result<PathBuf, String> {
    let _guard = CATALOG_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let candidates = codex_candidates();
    let target = catalog_path();
    let metadata = meta_path();
    ensure_catalog_from(&candidates, &target, &metadata)
}

/// 用给定候选列表确保目录可用。独立出来让磁盘缓存策略可以用真实文件做单测。
fn ensure_catalog_from(
    candidates: &[PathBuf],
    target: &Path,
    metadata: &Path,
) -> Result<PathBuf, String> {
    ensure_catalog_from_with_writer(candidates, target, metadata, atomic_write)
}

fn ensure_catalog_from_with_writer<F>(
    candidates: &[PathBuf],
    target: &Path,
    metadata: &Path,
    write_catalog: F,
) -> Result<PathBuf, String>
where
    F: FnMut(&Path, &[u8]) -> Result<(), String>,
{
    ensure_catalog_from_with_writers(candidates, target, metadata, write_catalog, atomic_write)
}

fn ensure_catalog_from_with_writers<C, M>(
    candidates: &[PathBuf],
    target: &Path,
    metadata: &Path,
    mut write_catalog: C,
    mut write_metadata: M,
) -> Result<PathBuf, String>
where
    C: FnMut(&Path, &[u8]) -> Result<(), String>,
    M: FnMut(&Path, &[u8]) -> Result<(), String>,
{
    let fingerprints_by_candidate: Vec<Option<String>> = candidates
        .iter()
        .map(|candidate| fingerprint(candidate))
        .collect();
    let current_candidate_fingerprints: Vec<String> = fingerprints_by_candidate
        .iter()
        .flatten()
        .cloned()
        .collect();
    let cached_is_valid = fs::read_to_string(target)
        .ok()
        .is_some_and(|text| is_valid_catalog_text(&text));

    if cached_is_valid {
        if let Ok(cached_metadata) = fs::read_to_string(metadata) {
            if serde_json::from_str::<CatalogMetadata>(&cached_metadata)
                .ok()
                .is_some_and(|cached| {
                    cached.candidate_fingerprints == current_candidate_fingerprints
                        && current_candidate_fingerprints
                            .iter()
                            .any(|current| current == &cached.source_fingerprint)
                })
            {
                return Ok(target.to_path_buf());
            }
        }
    }

    let mut last_failure = None;
    for (candidate, current_fingerprint) in candidates.iter().zip(fingerprints_by_candidate) {
        let Some(current_fingerprint) = current_fingerprint else {
            last_failure = Some(format!("无法读取候选指纹 {}", candidate.display()));
            continue;
        };
        let Some(json) = extract_from_binary(candidate) else {
            last_failure = Some(format!("候选不含有效模型目录 {}", candidate.display()));
            continue;
        };
        let serialized_metadata = serde_json::to_vec(&CatalogMetadata {
            source_fingerprint: current_fingerprint,
            candidate_fingerprints: current_candidate_fingerprints.clone(),
        })
        .map_err(|error| format!("序列化模型目录元数据失败: {error}"))?;

        let metadata_exists = match fs::symlink_metadata(metadata) {
            Ok(_) => true,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
            Err(error) => {
                last_failure = Some(format!(
                    "检查模型目录元数据失败 {}: {error}",
                    metadata.display()
                ));
                continue;
            }
        };
        if metadata_exists {
            if let Err(error) = write_metadata(metadata, b"") {
                last_failure = Some(format!(
                    "使模型目录元数据失效失败 {}: {error}",
                    metadata.display()
                ));
                continue;
            }
        }

        if let Err(error) = write_catalog(target, json.as_bytes()) {
            last_failure = Some(format!("写入模型目录失败 {}: {error}", target.display()));
            continue;
        }
        // 正确元数据写失败不致命:预先写入的空标记仍在,下次会重提一遍。
        let _ = write_metadata(metadata, &serialized_metadata);
        return Ok(target.to_path_buf());
    }

    if fs::read_to_string(target)
        .ok()
        .is_some_and(|text| is_valid_catalog_text(&text))
    {
        return Ok(target.to_path_buf());
    }

    let detail = last_failure
        .map(|failure| format!(";最后失败:{failure}"))
        .unwrap_or_default();
    Err(format!(
        "无法生成 Codex 模型目录:检查了 {} 个候选二进制,且没有可用的有效缓存{detail}",
        candidates.len(),
    ))
}

/// 在目标目录创建临时文件,完整同步后用同文件系统 rename 原子替换目标。
fn atomic_write(target: &Path, bytes: &[u8]) -> Result<(), String> {
    atomic_write_with_persist(target, bytes, |temp, target| {
        temp.persist(target)
            .map(|_| ())
            .map_err(|error| format!("原子替换目录文件失败 {}: {}", target.display(), error.error))
    })
}

fn atomic_write_with_persist<F>(target: &Path, bytes: &[u8], persist: F) -> Result<(), String>
where
    F: FnOnce(NamedTempFile, &Path) -> Result<(), String>,
{
    let parent = target
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)
        .map_err(|error| format!("创建目录失败 {}: {error}", parent.display()))?;

    let mut temp = NamedTempFile::new_in(parent)
        .map_err(|error| format!("创建临时目录文件失败 {}: {error}", parent.display()))?;
    temp.write_all(bytes)
        .map_err(|error| format!("写入临时目录文件失败: {error}"))?;
    temp.as_file()
        .sync_all()
        .map_err(|error| format!("同步临时目录文件失败: {error}"))?;
    persist(temp, target)?;
    Ok(())
}

/// 二进制指纹:路径 + 大小 + 纳秒精度 mtime。避免同秒、同大小的原地替换误命中缓存。
fn fingerprint(binary: &Path) -> Option<String> {
    let meta = fs::metadata(binary).ok()?;
    let mtime_nanos = meta
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_nanos();
    Some(format!(
        "{}|{}|{}",
        binary.display(),
        meta.len(),
        mtime_nanos
    ))
}

/// 定位 Codex CLI 二进制。桌面应用是靠它跑的(config.toml 里的 `CODEX_CLI_PATH` 即指向它),
/// 所以它内置的目录就是 Codex 实际使用的那份。
#[cfg(test)]
fn locate_codex_cli() -> Option<PathBuf> {
    codex_candidates().into_iter().next()
}

#[derive(Clone, Copy)]
enum NpmGlobalLayout {
    Windows,
    Unix,
}

/// 官方 `@openai/codex` 可选平台包的安装布局。
#[derive(Clone, Copy)]
struct CodexTargetLayout {
    npm_layout: NpmGlobalLayout,
    platform_package: &'static str,
    target_triple: &'static str,
    exe: &'static str,
}

/// 当前编译目标对应的平台包。未知目标仍保留普通 PATH / 桌面应用候选。
fn codex_target_layout() -> Option<CodexTargetLayout> {
    if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
        Some(CodexTargetLayout {
            npm_layout: NpmGlobalLayout::Windows,
            platform_package: "@openai/codex-win32-x64",
            target_triple: "x86_64-pc-windows-msvc",
            exe: "codex.exe",
        })
    } else if cfg!(all(target_os = "windows", target_arch = "aarch64")) {
        Some(CodexTargetLayout {
            npm_layout: NpmGlobalLayout::Windows,
            platform_package: "@openai/codex-win32-arm64",
            target_triple: "aarch64-pc-windows-msvc",
            exe: "codex.exe",
        })
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        Some(CodexTargetLayout {
            npm_layout: NpmGlobalLayout::Unix,
            platform_package: "@openai/codex-linux-x64",
            target_triple: "x86_64-unknown-linux-musl",
            exe: "codex",
        })
    } else if cfg!(all(target_os = "linux", target_arch = "aarch64")) {
        Some(CodexTargetLayout {
            npm_layout: NpmGlobalLayout::Unix,
            platform_package: "@openai/codex-linux-arm64",
            target_triple: "aarch64-unknown-linux-musl",
            exe: "codex",
        })
    } else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
        Some(CodexTargetLayout {
            npm_layout: NpmGlobalLayout::Unix,
            platform_package: "@openai/codex-darwin-x64",
            target_triple: "x86_64-apple-darwin",
            exe: "codex",
        })
    } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        Some(CodexTargetLayout {
            npm_layout: NpmGlobalLayout::Unix,
            platform_package: "@openai/codex-darwin-arm64",
            target_triple: "aarch64-apple-darwin",
            exe: "codex",
        })
    } else {
        None
    }
}

#[cfg(any(target_os = "linux", test))]
fn linux_codex_bin_root(home: &Path, xdg_data_home: Option<&std::ffi::OsStr>) -> PathBuf {
    let data_home = xdg_data_home
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".local").join("share"));
    data_home.join("OpenAI").join("Codex").join("bin")
}

/// 收集生产环境里所有可能的 Codex CLI,让提取层逐个尝试而不是只信第一个。
fn codex_candidates() -> Vec<PathBuf> {
    let target_layout = codex_target_layout();
    let exe = target_layout
        .map(|layout| layout.exe)
        .unwrap_or(if cfg!(windows) { "codex.exe" } else { "codex" });

    let mut roots: Vec<PathBuf> = Vec::new();
    #[cfg(windows)]
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        roots.push(
            PathBuf::from(local)
                .join("OpenAI")
                .join("Codex")
                .join("bin"),
        );
    }
    #[cfg(target_os = "macos")]
    roots.push(quotio_platform::expand_home_path(
        "~/Library/Application Support/OpenAI/Codex/bin",
    ));
    #[cfg(target_os = "linux")]
    roots.push(linux_codex_bin_root(
        &quotio_platform::home_dir(),
        std::env::var_os("XDG_DATA_HOME").as_deref(),
    ));
    roots.push(quotio_platform::expand_home_path("~/.codex/bin"));

    let app_path = crate::codex_launch::detect_codex_app_path_cached();
    let path_dirs: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|path| std::env::split_paths(&path).collect())
        .unwrap_or_default();

    target_layout.map_or_else(
        || collect_codex_candidates(app_path.as_deref(), &roots, &path_dirs, exe),
        |layout| {
            collect_codex_candidates_for_target(app_path.as_deref(), &roots, &path_dirs, layout)
        },
    )
}

/// 收集所有已知安装布局里的 Codex CLI,按 mtime 从新到旧排列。
fn collect_codex_candidates(
    app_path: Option<&Path>,
    roots: &[PathBuf],
    path_dirs: &[PathBuf],
    exe: &str,
) -> Vec<PathBuf> {
    collect_codex_candidates_with_target(app_path, roots, path_dirs, exe, None)
}

/// 和生产收集器相同,但由调用方显式给出目标布局,便于跨平台测试 npm 安装结构。
fn collect_codex_candidates_for_target(
    app_path: Option<&Path>,
    roots: &[PathBuf],
    path_dirs: &[PathBuf],
    target: CodexTargetLayout,
) -> Vec<PathBuf> {
    collect_codex_candidates_with_target(app_path, roots, path_dirs, target.exe, Some(target))
}

fn collect_codex_candidates_with_target(
    app_path: Option<&Path>,
    roots: &[PathBuf],
    path_dirs: &[PathBuf],
    exe: &str,
    target: Option<CodexTargetLayout>,
) -> Vec<PathBuf> {
    let mut paths = Vec::new();

    if let Some(app_path) = app_path {
        if let Some(app_dir) = app_path.parent() {
            // WindowsApps: app/Codex.exe + app/resources/codex.exe。
            paths.push(app_dir.join("resources").join(exe));

            // macOS: Codex.app/Contents/MacOS/Codex + Contents/Resources/codex。
            if app_dir
                .file_name()
                .is_some_and(|name| name.eq_ignore_ascii_case("MacOS"))
            {
                if let Some(contents_dir) = app_dir.parent() {
                    paths.push(contents_dir.join("Resources").join(exe));
                }
            }
        }
    }

    paths.extend(path_dirs.iter().map(|dir| dir.join(exe)));
    if let Some(target) = target {
        for path_dir in path_dirs {
            if let Some(package_root) = npm_codex_package_root(path_dir, target.npm_layout) {
                paths.extend(npm_native_candidates(&package_root, target));
            }
        }
    }

    for root in roots {
        paths.push(root.join(exe));
        if let Ok(entries) = fs::read_dir(root) {
            paths.extend(entries.flatten().map(|entry| entry.path().join(exe)));
        }
    }

    let mut candidates: Vec<(std::time::SystemTime, PathBuf)> = paths
        .into_iter()
        .filter(|path| path.is_file())
        .map(|path| {
            let modified = fs::metadata(&path)
                .and_then(|meta| meta.modified())
                .unwrap_or(std::time::UNIX_EPOCH);
            (modified, path)
        })
        .collect();
    candidates.sort_by(|(left_time, left_path), (right_time, right_path)| {
        right_time
            .cmp(left_time)
            // read_dir 在相同 mtime 下没有稳定顺序,路径保证元数据快照可复现。
            .then_with(|| left_path.cmp(right_path))
    });

    let mut seen = HashSet::new();
    candidates
        .into_iter()
        .filter_map(|(_, path)| {
            let canonical = fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
            seen.insert(canonical).then_some(path)
        })
        .collect()
}

fn npm_codex_package_root(path_dir: &Path, layout: NpmGlobalLayout) -> Option<PathBuf> {
    let node_modules = match layout {
        NpmGlobalLayout::Windows => path_dir.join("node_modules"),
        NpmGlobalLayout::Unix => path_dir.parent()?.join("lib").join("node_modules"),
    };
    Some(npm_package_path(&node_modules, "@openai/codex"))
}

fn npm_native_candidates(codex_package_root: &Path, target: CodexTargetLayout) -> Vec<PathBuf> {
    let mut package_roots = vec![npm_package_path(
        &codex_package_root.join("node_modules"),
        target.platform_package,
    )];
    if let Some(node_modules) = codex_package_root.parent().and_then(Path::parent) {
        package_roots.push(npm_package_path(node_modules, target.platform_package));
    }
    package_roots.push(codex_package_root.to_path_buf());

    package_roots
        .into_iter()
        .map(|package_root| {
            package_root
                .join("vendor")
                .join(target.target_triple)
                .join("bin")
                .join(target.exe)
        })
        .collect()
}

fn npm_package_path(node_modules: &Path, package: &str) -> PathBuf {
    package
        .split('/')
        .filter(|component| !component.is_empty())
        .fold(node_modules.to_path_buf(), |path, component| {
            path.join(component)
        })
}

/// 在二进制里分块搜锚点(不能把几百 MB 整个读进内存),命中后只取锚点附近一块做解析。
fn extract_from_binary(binary: &Path) -> Option<String> {
    let mut file = fs::File::open(binary).ok()?;
    let len = file.metadata().ok()?.len();

    const CHUNK: usize = 8 * 1024 * 1024;
    let overlap = ANCHOR.len().saturating_sub(1);
    let mut buf = vec![0u8; CHUNK];
    let mut base: u64 = 0;
    let mut anchor_at: Option<u64> = None;

    while base < len {
        file.seek(SeekFrom::Start(base)).ok()?;
        let n = read_fill(&mut file, &mut buf)?;
        if n == 0 {
            break;
        }
        if let Some(i) = find(&buf[..n], ANCHOR) {
            anchor_at = Some(base + i as u64);
            break;
        }
        if n <= overlap {
            break;
        }
        base += (n - overlap) as u64;
    }

    let anchor_at = anchor_at?;
    // 取 [anchor-BACK_WINDOW, anchor+FORWARD_WINDOW) 这一段,足以覆盖整份目录。
    let start = anchor_at.saturating_sub(BACK_WINDOW as u64);
    let want = (anchor_at - start) as usize + FORWARD_WINDOW;
    file.seek(SeekFrom::Start(start)).ok()?;
    let mut window = vec![0u8; want.min((len - start) as usize)];
    let got = read_fill(&mut file, &mut window)?;
    window.truncate(got);

    let anchor_in_window = (anchor_at - start) as usize;
    extract_catalog_json(&window, anchor_in_window)
}

/// 尽量填满 buf(文件读取可能短读),返回实际读到的字节数。
fn read_fill(file: &mut fs::File, buf: &mut [u8]) -> Option<usize> {
    let mut filled = 0;
    while filled < buf.len() {
        match file.read(&mut buf[filled..]) {
            Ok(0) => break,
            Ok(n) => filled += n,
            Err(_) => return None,
        }
    }
    Some(filled)
}

/// SIMD 子串搜索。二进制有几百 MB,朴素的 `windows().position()` 实测要 8s+。
fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    memchr::memmem::find(haystack, needle)
}

/// 纯函数:给定一段含目录的字节和锚点位置,切出 `{"models":[…]}` 并校验。
/// 独立出来便于单测 —— 不必真的去啃一个几百 MB 的二进制。
fn extract_catalog_json(window: &[u8], anchor: usize) -> Option<String> {
    // 锚点往前找 `"models"`,再往前找包住它的 `{`。
    let back_from = anchor.saturating_sub(BACK_WINDOW);
    let models_at = rfind(&window[back_from..anchor], b"\"models\"")? + back_from;
    let open_at = rfind(&window[back_from..models_at], b"{")? + back_from;

    let end = match_braces(&window[open_at..])?;
    let raw = &window[open_at..open_at + end];

    let text = std::str::from_utf8(raw).ok()?;
    is_valid_catalog_text(text).then(|| text.to_string())
}

/// 提取结果和磁盘缓存共用同一条结构校验规则,避免只凭指纹信任损坏文件。
fn is_valid_catalog_text(text: &str) -> bool {
    let Ok(value) = serde_json::from_str::<Value>(text) else {
        return false;
    };
    let Some(models) = value.get("models").and_then(Value::as_array) else {
        return false;
    };
    // 空目录会被 Codex 拒:"must contain at least one model"。字段不对也别写出去。
    !models.is_empty()
        && models.iter().all(|model| {
            model
                .get("slug")
                .and_then(Value::as_str)
                .is_some_and(|slug| !slug.trim().is_empty())
                && model
                    .get("supported_reasoning_levels")
                    .and_then(Value::as_array)
                    .is_some_and(|levels| {
                        !levels.is_empty()
                            && levels.iter().all(|level| {
                                level
                                    .get("effort")
                                    .and_then(Value::as_str)
                                    .is_some_and(|effort| !effort.trim().is_empty())
                            })
                    })
        })
}

/// 只在锚点附近的小窗口(≤16KB)里反向搜,用 memmem 的 rfind。
fn rfind(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    memchr::memmem::rfind(haystack, needle)
}

/// 从 `bytes[0] == b'{'` 开始配平花括号,返回闭合括号之后的偏移。
/// **必须跳过字符串字面量**:模型描述里出现 `{` / `}` 会把朴素计数带偏。
fn match_braces(bytes: &[u8]) -> Option<usize> {
    if bytes.first() != Some(&b'{') {
        return None;
    }
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    for (i, &c) in bytes.iter().enumerate() {
        if in_string {
            if escaped {
                escaped = false;
            } else if c == b'\\' {
                escaped = true;
            } else if c == b'"' {
                in_string = false;
            }
            continue;
        }
        match c {
            b'"' => in_string = true,
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i + 1);
                }
            }
            _ => {}
        }
    }
    None
}

/// 写进 TOML 的路径:统一用正斜杠。Windows 反斜杠在 TOML 基本字符串里是转义符,
/// `"C:\Users\…"` 的 `\U` 是**非法转义**,会让整个 config.toml 解析失败。
pub fn toml_path_value(path: &Path) -> String {
    path.display().to_string().replace('\\', "/")
}

/// 某个模型支持的推理档位,按目录里的顺序(即从低到高)。
///
/// 让「思考程度」下拉**跟着目录走**而不是写死:不同模型档位不同(实测 gpt-5.6-sol/terra 有
/// low…ultra 六档,luna 五档,gpt-5.5 及更早只有四档),而且 Codex 以后加新档位时无需改 Quotio。
/// 目录取不到 / 模型不在目录里 → 返回空,调用方回退到内置的保守列表。
pub fn reasoning_levels(model_slug: &str) -> Vec<String> {
    reasoning_levels_result(model_slug).unwrap_or_default()
}

/// `reasoning_levels` 的可诊断版本,区分目录获取、读取和 JSON 解析错误。
pub fn reasoning_levels_result(model_slug: &str) -> Result<Vec<String>, String> {
    let slug = model_slug.trim();
    if slug.is_empty() {
        return Ok(Vec::new());
    }
    let path =
        ensure_catalog_result().map_err(|error| format!("获取 Codex 模型目录失败: {error}"))?;
    let text = fs::read_to_string(&path)
        .map_err(|error| format!("读取 Codex 模型目录失败 {}: {error}", path.display()))?;
    let value = serde_json::from_str::<Value>(&text)
        .map_err(|error| format!("解析 Codex 模型目录失败 {}: {error}", path.display()))?;
    Ok(value
        .get("models")
        .and_then(Value::as_array)
        .and_then(|models| {
            models
                .iter()
                .find(|m| m.get("slug").and_then(Value::as_str) == Some(slug))
        })
        .and_then(|model| model.get("supported_reasoning_levels")?.as_array())
        .map(|levels| {
            levels
                .iter()
                .filter_map(|level| level.get("effort")?.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestDir(PathBuf);

    impl TestDir {
        fn new(label: &str) -> Self {
            static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let unique = format!(
                "quotio-codex-catalog-{label}-{}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .expect("system clock should be after Unix epoch")
                    .as_nanos(),
                NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            );
            let path = std::env::temp_dir().join(unique);
            fs::create_dir(&path).expect("temporary test directory should be created");
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn catalog_bytes(inner: &str) -> Vec<u8> {
        format!("....junk....{inner}....trailing junk....").into_bytes()
    }

    const ONE_MODEL: &str = r#"{"models":[{"slug":"gpt-5.6-sol","supported_reasoning_levels":[{"effort":"low"},{"effort":"ultra"}]}]}"#;
    const NEW_MODEL: &str = r#"{"models":[{"slug":"gpt-5.7","supported_reasoning_levels":[{"effort":"medium"},{"effort":"max"}]}]}"#;

    fn metadata_json(source_fingerprint: &str, candidate_fingerprints: &[String]) -> String {
        serde_json::json!({
            "source_fingerprint": source_fingerprint,
            "candidate_fingerprints": candidate_fingerprints,
        })
        .to_string()
    }

    fn assert_metadata_bytes(
        bytes: &[u8],
        source_fingerprint: &str,
        candidate_fingerprints: &[String],
    ) {
        let value: Value = serde_json::from_slice(bytes).unwrap();
        assert_eq!(value["source_fingerprint"], source_fingerprint);
        assert_eq!(
            value["candidate_fingerprints"],
            serde_json::json!(candidate_fingerprints)
        );
    }

    fn assert_metadata(
        metadata: &Path,
        source_fingerprint: &str,
        candidate_fingerprints: &[String],
    ) {
        assert_metadata_bytes(
            &fs::read(metadata).unwrap(),
            source_fingerprint,
            candidate_fingerprints,
        );
    }

    #[test]
    fn extraction_continues_after_an_invalid_candidate() {
        let temp = TestDir::new("invalid-then-valid");
        let invalid = temp.path().join("invalid-codex.exe");
        let valid = temp.path().join("valid-codex.exe");
        let target = temp.path().join("catalog.json");
        let meta = temp.path().join("catalog.meta.json");
        fs::write(&invalid, b"binary without a model catalog").unwrap();
        fs::write(&valid, catalog_bytes(ONE_MODEL)).unwrap();
        let invalid_fingerprint = fingerprint(&invalid).unwrap();
        let valid_fingerprint = fingerprint(&valid).unwrap();

        let result = ensure_catalog_from(&[invalid, valid.clone()], &target, &meta)
            .expect("the valid fallback candidate should be extracted");

        assert_eq!(result, target);
        assert_eq!(fs::read_to_string(&target).unwrap(), ONE_MODEL);
        assert_metadata(
            &meta,
            &valid_fingerprint,
            &[invalid_fingerprint, valid_fingerprint.clone()],
        );
    }

    #[test]
    fn newer_candidate_invalidates_cache_recorded_with_only_old_candidate() {
        let temp = TestDir::new("new-candidate-invalidates-cache");
        let old_candidate = temp.path().join("old-codex.exe");
        let new_candidate = temp.path().join("new-codex.exe");
        let target = temp.path().join("catalog.json");
        let meta = temp.path().join("catalog.meta.json");
        fs::write(&old_candidate, catalog_bytes(ONE_MODEL)).unwrap();
        fs::write(&new_candidate, catalog_bytes(NEW_MODEL)).unwrap();
        fs::write(&target, ONE_MODEL).unwrap();

        let old_fingerprint = fingerprint(&old_candidate).unwrap();
        let new_fingerprint = fingerprint(&new_candidate).unwrap();
        fs::write(
            &meta,
            metadata_json(&old_fingerprint, std::slice::from_ref(&old_fingerprint)),
        )
        .unwrap();

        ensure_catalog_from(&[new_candidate.clone(), old_candidate], &target, &meta)
            .expect("adding a higher-priority candidate should regenerate the catalog");

        assert_eq!(fs::read_to_string(&target).unwrap(), NEW_MODEL);
        assert_metadata(
            &meta,
            &new_fingerprint,
            &[new_fingerprint.clone(), old_fingerprint],
        );
    }

    #[test]
    fn unchanged_candidates_use_cached_lower_valid_source() {
        let temp = TestDir::new("cached-lower-valid-source");
        let invalid = temp.path().join("invalid-codex.exe");
        let valid = temp.path().join("valid-codex.exe");
        let target = temp.path().join("catalog.json");
        let meta = temp.path().join("catalog.meta.json");
        fs::write(&invalid, b"binary without a model catalog").unwrap();
        fs::write(&valid, catalog_bytes(ONE_MODEL)).unwrap();
        fs::write(&target, ONE_MODEL).unwrap();

        let invalid_fingerprint = fingerprint(&invalid).unwrap();
        let valid_fingerprint = fingerprint(&valid).unwrap();
        fs::write(
            &meta,
            metadata_json(
                &valid_fingerprint,
                &[invalid_fingerprint, valid_fingerprint.clone()],
            ),
        )
        .unwrap();

        let mut catalog_writes = 0;
        let mut metadata_writes = 0;
        ensure_catalog_from_with_writers(
            &[invalid, valid],
            &target,
            &meta,
            |path, bytes| {
                catalog_writes += 1;
                atomic_write(path, bytes)
            },
            |path, bytes| {
                metadata_writes += 1;
                atomic_write(path, bytes)
            },
        )
        .expect("an unchanged candidate list should use the lower valid cached source");

        assert_eq!(catalog_writes, 0);
        assert_eq!(metadata_writes, 0);
    }

    #[test]
    fn legacy_raw_fingerprint_metadata_is_regenerated() {
        let temp = TestDir::new("legacy-raw-metadata");
        let valid = temp.path().join("valid-codex.exe");
        let target = temp.path().join("catalog.json");
        let meta = temp.path().join("catalog.meta.json");
        fs::write(&valid, catalog_bytes(ONE_MODEL)).unwrap();
        fs::write(&target, ONE_MODEL).unwrap();
        let valid_fingerprint = fingerprint(&valid).unwrap();
        fs::write(&meta, &valid_fingerprint).unwrap();

        let mut catalog_writes = 0;
        let mut metadata_writes = 0;
        ensure_catalog_from_with_writers(
            std::slice::from_ref(&valid),
            &target,
            &meta,
            |path, bytes| {
                catalog_writes += 1;
                atomic_write(path, bytes)
            },
            |path, bytes| {
                metadata_writes += 1;
                atomic_write(path, bytes)
            },
        )
        .expect("legacy metadata should be stale rather than fatal");

        assert_eq!(catalog_writes, 1);
        assert_eq!(metadata_writes, 2);
        assert_metadata(
            &meta,
            &valid_fingerprint,
            std::slice::from_ref(&valid_fingerprint),
        );
    }

    #[test]
    fn seconds_precision_structured_metadata_is_regenerated() {
        let temp = TestDir::new("seconds-precision-metadata");
        let valid = temp.path().join("valid-codex.exe");
        let target = temp.path().join("catalog.json");
        let meta = temp.path().join("catalog.meta.json");
        fs::write(&valid, catalog_bytes(ONE_MODEL)).unwrap();
        fs::write(&target, ONE_MODEL).unwrap();

        let binary_metadata = fs::metadata(&valid).unwrap();
        let legacy_mtime = binary_metadata
            .modified()
            .unwrap()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let legacy_fingerprint = format!(
            "{}|{}|{}",
            valid.display(),
            binary_metadata.len(),
            legacy_mtime
        );
        fs::write(
            &meta,
            metadata_json(
                &legacy_fingerprint,
                std::slice::from_ref(&legacy_fingerprint),
            ),
        )
        .unwrap();

        let mut catalog_writes = 0;
        let mut metadata_writes = 0;
        ensure_catalog_from_with_writers(
            std::slice::from_ref(&valid),
            &target,
            &meta,
            |path, bytes| {
                catalog_writes += 1;
                atomic_write(path, bytes)
            },
            |path, bytes| {
                metadata_writes += 1;
                atomic_write(path, bytes)
            },
        )
        .expect("seconds-precision structured metadata should be regenerated");

        let current_fingerprint = fingerprint(&valid).unwrap();
        assert_eq!(catalog_writes, 1);
        assert_eq!(metadata_writes, 2);
        assert_ne!(current_fingerprint, legacy_fingerprint);
        assert_metadata(
            &meta,
            &current_fingerprint,
            std::slice::from_ref(&current_fingerprint),
        );
    }

    #[test]
    fn cached_source_must_belong_to_candidate_snapshot() {
        let temp = TestDir::new("source-not-in-candidates");
        let valid = temp.path().join("valid-codex.exe");
        let target = temp.path().join("catalog.json");
        let meta = temp.path().join("catalog.meta.json");
        fs::write(&valid, catalog_bytes(ONE_MODEL)).unwrap();
        fs::write(&target, ONE_MODEL).unwrap();
        let valid_fingerprint = fingerprint(&valid).unwrap();
        fs::write(
            &meta,
            metadata_json(
                "fingerprint-not-in-candidate-list",
                std::slice::from_ref(&valid_fingerprint),
            ),
        )
        .unwrap();

        let mut catalog_writes = 0;
        let mut metadata_writes = 0;
        ensure_catalog_from_with_writers(
            std::slice::from_ref(&valid),
            &target,
            &meta,
            |path, bytes| {
                catalog_writes += 1;
                atomic_write(path, bytes)
            },
            |path, bytes| {
                metadata_writes += 1;
                atomic_write(path, bytes)
            },
        )
        .expect("metadata with an impossible source should be regenerated");

        assert_eq!(catalog_writes, 1);
        assert_eq!(metadata_writes, 2);
        assert_metadata(
            &meta,
            &valid_fingerprint,
            std::slice::from_ref(&valid_fingerprint),
        );
    }

    #[test]
    fn failed_fingerprint_update_leaves_metadata_invalid_for_retry() {
        let temp = TestDir::new("failed-fingerprint-update");
        let old_candidate = temp.path().join("old-codex.exe");
        let new_candidate = temp.path().join("new-codex.exe");
        let target = temp.path().join("catalog.json");
        let meta = temp.path().join("catalog.meta.json");
        fs::write(&old_candidate, catalog_bytes(ONE_MODEL)).unwrap();
        fs::write(&new_candidate, catalog_bytes(NEW_MODEL)).unwrap();
        fs::write(&target, ONE_MODEL).unwrap();

        let old_fingerprint = fingerprint(&old_candidate).unwrap();
        let new_fingerprint = fingerprint(&new_candidate).unwrap();
        fs::write(
            &meta,
            metadata_json(&old_fingerprint, std::slice::from_ref(&old_fingerprint)),
        )
        .unwrap();

        let mut catalog_writes = 0;
        let mut metadata_writes = 0;
        let result = ensure_catalog_from_with_writers(
            std::slice::from_ref(&new_candidate),
            &target,
            &meta,
            |path, bytes| {
                catalog_writes += 1;
                assert_eq!(
                    fs::read(&meta).unwrap(),
                    b"",
                    "metadata must be invalidated before catalog replacement"
                );
                atomic_write(path, bytes)
            },
            |path, bytes| {
                metadata_writes += 1;
                match metadata_writes {
                    1 => {
                        assert!(bytes.is_empty(), "first metadata write must invalidate");
                        assert_eq!(fs::read_to_string(&target).unwrap(), ONE_MODEL);
                        atomic_write(path, bytes)
                    }
                    2 => {
                        assert_metadata_bytes(
                            bytes,
                            &new_fingerprint,
                            std::slice::from_ref(&new_fingerprint),
                        );
                        assert_eq!(fs::read_to_string(&target).unwrap(), NEW_MODEL);
                        Err("simulated final fingerprint write failure".to_string())
                    }
                    _ => panic!("unexpected metadata write #{metadata_writes}"),
                }
            },
        )
        .expect("the valid replacement catalog should remain usable");

        assert_eq!(result, target);
        assert_eq!(catalog_writes, 1);
        assert_eq!(metadata_writes, 2);
        assert_eq!(fs::read_to_string(&target).unwrap(), NEW_MODEL);
        assert_eq!(fs::read(&meta).unwrap(), b"");
        assert_ne!(fs::read_to_string(&meta).unwrap(), old_fingerprint);

        let mut retry_catalog_writes = 0;
        let mut retry_metadata_writes = 0;
        ensure_catalog_from_with_writers(
            &[new_candidate, old_candidate],
            &target,
            &meta,
            |path, bytes| {
                retry_catalog_writes += 1;
                atomic_write(path, bytes)
            },
            |path, bytes| {
                retry_metadata_writes += 1;
                atomic_write(path, bytes)
            },
        )
        .expect("invalid metadata should force a retry instead of the cache fast path");

        assert_eq!(retry_catalog_writes, 1);
        assert!(retry_metadata_writes > 0);
    }

    #[test]
    fn metadata_invalidation_failure_prevents_catalog_replacement() {
        let temp = TestDir::new("metadata-invalidation-failure");
        let old_candidate = temp.path().join("old-codex.exe");
        let new_candidate = temp.path().join("new-codex.exe");
        let target = temp.path().join("catalog.json");
        let meta = temp.path().join("catalog.meta.json");
        fs::write(&old_candidate, catalog_bytes(ONE_MODEL)).unwrap();
        fs::write(&new_candidate, catalog_bytes(NEW_MODEL)).unwrap();
        fs::write(&target, ONE_MODEL).unwrap();
        let old_fingerprint = fingerprint(&old_candidate).unwrap();
        fs::write(
            &meta,
            metadata_json(&old_fingerprint, std::slice::from_ref(&old_fingerprint)),
        )
        .unwrap();

        let mut catalog_writes = 0;
        let mut metadata_writes = 0;
        let result = ensure_catalog_from_with_writers(
            std::slice::from_ref(&new_candidate),
            &target,
            &meta,
            |path, bytes| {
                catalog_writes += 1;
                atomic_write(path, bytes)
            },
            |_, _| {
                metadata_writes += 1;
                Err("simulated metadata invalidation failure".to_string())
            },
        )
        .expect("the valid old catalog should remain the last-known-good fallback");

        assert_eq!(result, target);
        assert_eq!(metadata_writes, 1);
        assert_eq!(catalog_writes, 0);
        assert_eq!(fs::read_to_string(&target).unwrap(), ONE_MODEL);
        assert_metadata(
            &meta,
            &old_fingerprint,
            std::slice::from_ref(&old_fingerprint),
        );
    }

    #[test]
    fn valid_existing_catalog_survives_when_all_candidates_fail() {
        let temp = TestDir::new("last-known-good");
        let invalid = temp.path().join("invalid-codex.exe");
        let target = temp.path().join("catalog.json");
        let meta = temp.path().join("catalog.meta.json");
        fs::write(&invalid, b"binary without a model catalog").unwrap();
        fs::write(&target, ONE_MODEL).unwrap();

        let result = ensure_catalog_from(std::slice::from_ref(&invalid), &target, &meta)
            .expect("a valid existing catalog should be retained");

        assert_eq!(result, target);
        assert_eq!(fs::read_to_string(&target).unwrap(), ONE_MODEL);
        assert!(!meta.exists(), "fallback must not invent new metadata");
    }

    #[test]
    fn write_failures_try_every_candidate_and_preserve_last_known_good() {
        let temp = TestDir::new("write-failure-fallback");
        let first = temp.path().join("first-codex.exe");
        let second = temp.path().join("second-codex.exe");
        let target = temp.path().join("catalog.json");
        let meta = temp.path().join("catalog.meta.json");
        fs::write(&first, catalog_bytes(ONE_MODEL)).unwrap();
        fs::write(&second, catalog_bytes(ONE_MODEL)).unwrap();
        fs::write(&target, ONE_MODEL).unwrap();
        let mut write_attempts = 0;

        let result = ensure_catalog_from_with_writer(&[first, second], &target, &meta, |_, _| {
            write_attempts += 1;
            Err("simulated catalog write failure".to_string())
        })
        .expect("the valid existing catalog should survive replacement failures");

        assert_eq!(write_attempts, 2);
        assert_eq!(result, target);
        assert_eq!(fs::read_to_string(&target).unwrap(), ONE_MODEL);
        assert!(!meta.exists(), "failed writes must not invent metadata");
    }

    #[test]
    fn malformed_cached_catalog_is_not_trusted_even_when_meta_matches() {
        let temp = TestDir::new("malformed-cache");
        let valid = temp.path().join("valid-codex.exe");
        let target = temp.path().join("catalog.json");
        let meta = temp.path().join("catalog.meta.json");
        fs::write(&valid, catalog_bytes(ONE_MODEL)).unwrap();
        fs::write(&target, r#"{"models":[]}"#).unwrap();
        let valid_fingerprint = fingerprint(&valid).unwrap();
        fs::write(
            &meta,
            metadata_json(&valid_fingerprint, std::slice::from_ref(&valid_fingerprint)),
        )
        .unwrap();

        let result = ensure_catalog_from(std::slice::from_ref(&valid), &target, &meta)
            .expect("the malformed cache should be replaced from the valid candidate");

        assert_eq!(result, target);
        assert_eq!(fs::read_to_string(&target).unwrap(), ONE_MODEL);
    }

    #[test]
    fn atomic_write_replaces_existing_content() {
        let temp = TestDir::new("atomic-replace");
        let target = temp.path().join("catalog.json");
        fs::write(&target, b"old").unwrap();

        atomic_write(&target, b"new catalog").expect("atomic replacement should succeed");

        assert_eq!(fs::read(&target).unwrap(), b"new catalog");
    }

    #[test]
    fn atomic_write_preserves_existing_content_when_persist_fails() {
        let temp = TestDir::new("atomic-persist-failure");
        let target = temp.path().join("catalog.json");
        fs::write(&target, b"known old catalog bytes").unwrap();
        let mut persist_attempts = 0;

        let error = atomic_write_with_persist(
            &target,
            b"replacement catalog bytes",
            |temp: NamedTempFile, persist_target: &Path| {
                persist_attempts += 1;
                assert_eq!(persist_target, target.as_path());
                assert_eq!(temp.path().parent(), target.parent());
                assert_eq!(fs::read(temp.path()).unwrap(), b"replacement catalog bytes");
                Err("simulated persist failure".to_string())
            },
        )
        .expect_err("persist failure should be returned");

        assert_eq!(persist_attempts, 1);
        assert!(error.contains("simulated persist failure"));
        assert_eq!(fs::read(&target).unwrap(), b"known old catalog bytes");
    }

    #[test]
    fn missing_catalog_reports_the_candidate_count() {
        let temp = TestDir::new("candidate-count");
        let first = temp.path().join("first-codex.exe");
        let second = temp.path().join("second-codex.exe");
        let target = temp.path().join("catalog.json");
        let meta = temp.path().join("catalog.meta.json");
        fs::write(&first, b"no catalog here").unwrap();
        fs::write(&second, b"still no catalog here").unwrap();

        let error = ensure_catalog_from(&[first, second], &target, &meta)
            .expect_err("missing extraction and cache should be diagnostic");

        assert!(
            error.contains("2 个候选"),
            "candidate count should be reported: {error}"
        );
    }

    #[test]
    fn empty_model_slug_is_an_empty_success() {
        assert_eq!(reasoning_levels_result("  ").unwrap(), Vec::<String>::new());
    }

    #[test]
    fn discovers_windows_app_resource_without_local_bin_cache() {
        let temp = TestDir::new("windows-app-resource");
        let app_dir = temp.path().join("app");
        let app = app_dir.join("Codex.exe");
        let resource_cli = app_dir.join("resources").join("codex.exe");
        fs::create_dir_all(resource_cli.parent().unwrap()).unwrap();
        fs::write(&app, b"desktop launcher").unwrap();
        fs::write(&resource_cli, b"embedded codex cli").unwrap();

        let candidates = collect_codex_candidates(Some(&app), &[], &[], "codex.exe");

        assert!(
            candidates.contains(&resource_cli),
            "desktop resource CLI should be discovered: {candidates:?}"
        );
    }

    #[test]
    fn discovers_windows_npm_nested_native_binary_from_path_prefix() {
        let temp = TestDir::new("windows-npm-layout");
        let path_dir = temp.path().join("npm");
        let generic_package = path_dir.join("node_modules").join("@openai").join("codex");
        let native = generic_package
            .join("node_modules")
            .join("@openai")
            .join("codex-win32-x64")
            .join("vendor")
            .join("x86_64-pc-windows-msvc")
            .join("bin")
            .join("codex.exe");
        fs::create_dir_all(native.parent().unwrap()).unwrap();
        fs::write(path_dir.join("codex.cmd"), b"official npm shim").unwrap();
        fs::write(&native, b"native codex binary").unwrap();

        let candidates = collect_codex_candidates_for_target(
            None,
            &[],
            std::slice::from_ref(&path_dir),
            CodexTargetLayout {
                npm_layout: NpmGlobalLayout::Windows,
                platform_package: "@openai/codex-win32-x64",
                target_triple: "x86_64-pc-windows-msvc",
                exe: "codex.exe",
            },
        );

        assert!(
            candidates.contains(&native),
            "nested Windows npm native binary should be discovered: {candidates:?}"
        );
    }

    #[test]
    fn discovers_unix_npm_hoisted_native_binary_without_symlinks() {
        let temp = TestDir::new("unix-npm-hoisted-layout");
        let prefix = temp.path().join("prefix");
        let path_dir = prefix.join("bin");
        let generic_package = prefix
            .join("lib")
            .join("node_modules")
            .join("@openai")
            .join("codex");
        let native = prefix
            .join("lib")
            .join("node_modules")
            .join("@openai")
            .join("codex-linux-x64")
            .join("vendor")
            .join("x86_64-unknown-linux-musl")
            .join("bin")
            .join("codex");
        fs::create_dir_all(&path_dir).unwrap();
        fs::create_dir_all(&generic_package).unwrap();
        fs::create_dir_all(native.parent().unwrap()).unwrap();
        fs::write(&native, b"native codex binary").unwrap();

        let candidates = collect_codex_candidates_for_target(
            None,
            &[],
            std::slice::from_ref(&path_dir),
            CodexTargetLayout {
                npm_layout: NpmGlobalLayout::Unix,
                platform_package: "@openai/codex-linux-x64",
                target_triple: "x86_64-unknown-linux-musl",
                exe: "codex",
            },
        );

        assert!(
            candidates.contains(&native),
            "hoisted Unix npm native binary should be discovered: {candidates:?}"
        );
    }

    #[test]
    fn discovers_generic_codex_package_vendor_fallback() {
        let temp = TestDir::new("npm-generic-vendor-layout");
        let prefix = temp.path().join("prefix");
        let path_dir = prefix.join("bin");
        let native = prefix
            .join("lib")
            .join("node_modules")
            .join("@openai")
            .join("codex")
            .join("vendor")
            .join("x86_64-unknown-linux-musl")
            .join("bin")
            .join("codex");
        fs::create_dir_all(&path_dir).unwrap();
        fs::create_dir_all(native.parent().unwrap()).unwrap();
        fs::write(&native, b"native codex binary").unwrap();

        let candidates = collect_codex_candidates_for_target(
            None,
            &[],
            std::slice::from_ref(&path_dir),
            CodexTargetLayout {
                npm_layout: NpmGlobalLayout::Unix,
                platform_package: "@openai/codex-linux-x64",
                target_triple: "x86_64-unknown-linux-musl",
                exe: "codex",
            },
        );

        assert!(
            candidates.contains(&native),
            "generic package vendor fallback should be discovered: {candidates:?}"
        );
    }

    #[test]
    fn linux_codex_bin_root_honors_xdg_and_falls_back_for_missing_or_empty_values() {
        let temp = TestDir::new("linux-xdg-data-home");
        let home = temp.path().join("home");
        let xdg = temp.path().join("xdg-data");
        let fallback = home
            .join(".local")
            .join("share")
            .join("OpenAI")
            .join("Codex")
            .join("bin");

        assert_eq!(
            linux_codex_bin_root(&home, Some(xdg.as_os_str())),
            xdg.join("OpenAI").join("Codex").join("bin")
        );
        assert_eq!(linux_codex_bin_root(&home, None), fallback);
        assert_eq!(
            linux_codex_bin_root(&home, Some(std::ffi::OsStr::new(""))),
            fallback
        );
    }

    #[test]
    fn newer_hashed_binary_is_not_hidden_by_old_direct_binary() {
        let temp = TestDir::new("newer-hashed-binary");
        let root = temp.path().join("bin");
        let direct = root.join("codex.exe");
        let newer = root.join("new-version").join("codex.exe");
        fs::create_dir_all(newer.parent().unwrap()).unwrap();

        let direct_file = fs::File::create(&direct).unwrap();
        direct_file
            .set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(1))
            .unwrap();
        let newer_file = fs::File::create(&newer).unwrap();
        newer_file
            .set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(2))
            .unwrap();

        let candidates =
            collect_codex_candidates(None, std::slice::from_ref(&root), &[], "codex.exe");

        assert_eq!(candidates.first(), Some(&newer));
        assert!(
            candidates.contains(&direct),
            "direct binary should remain as a fallback: {candidates:?}"
        );
    }

    #[test]
    fn equal_mtime_candidates_use_a_path_tiebreaker() {
        let temp = TestDir::new("equal-mtime-candidates");
        let first_dir = temp.path().join("a-bin");
        let second_dir = temp.path().join("b-bin");
        let first = first_dir.join("codex.exe");
        let second = second_dir.join("codex.exe");
        fs::create_dir_all(&first_dir).unwrap();
        fs::create_dir_all(&second_dir).unwrap();
        let same_mtime = std::time::UNIX_EPOCH + std::time::Duration::from_secs(3);
        let first_file = fs::File::create(&first).unwrap();
        first_file.set_modified(same_mtime).unwrap();
        let second_file = fs::File::create(&second).unwrap();
        second_file.set_modified(same_mtime).unwrap();

        let candidates = collect_codex_candidates(None, &[], &[second_dir, first_dir], "codex.exe");

        assert_eq!(candidates, vec![first, second]);
    }

    #[test]
    fn fingerprint_distinguishes_subsecond_mtime_changes() {
        let temp = TestDir::new("subsecond-fingerprint");
        let binary = temp.path().join("codex.exe");
        let bytes = b"fixed-size-binary";
        fs::write(&binary, bytes).unwrap();
        let file = fs::OpenOptions::new().write(true).open(&binary).unwrap();
        let first_mtime = std::time::UNIX_EPOCH + std::time::Duration::new(42, 100_000_000);
        let second_mtime = std::time::UNIX_EPOCH + std::time::Duration::new(42, 900_000_000);

        file.set_modified(first_mtime).unwrap();
        let first_fingerprint = fingerprint(&binary).unwrap();
        let first_len = fs::metadata(&binary).unwrap().len();

        file.set_modified(second_mtime).unwrap();
        let second_fingerprint = fingerprint(&binary).unwrap();
        let second_len = fs::metadata(&binary).unwrap().len();

        assert_eq!(first_len, bytes.len() as u64);
        assert_eq!(second_len, first_len);
        assert_eq!(binary, temp.path().join("codex.exe"));
        assert_ne!(first_fingerprint, second_fingerprint);
    }

    #[test]
    fn extracts_catalog_around_the_anchor() {
        let bytes = catalog_bytes(ONE_MODEL);
        let anchor = find(&bytes, ANCHOR).expect("锚点应存在");
        let json = extract_catalog_json(&bytes, anchor).expect("应提取出目录");
        let value: Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["models"][0]["slug"], "gpt-5.6-sol");
        assert_eq!(
            value["models"][0]["supported_reasoning_levels"][1]["effort"],
            "ultra"
        );
    }

    #[test]
    fn brace_matching_ignores_braces_inside_strings() {
        // 描述里塞一个 `}`:朴素计数会在这里提前收尾,切出残缺 JSON。
        let inner = r#"{"models":[{"slug":"m","description":"a } brace {","supported_reasoning_levels":[{"effort":"low"}]}]}"#;
        let bytes = catalog_bytes(inner);
        let anchor = find(&bytes, ANCHOR).unwrap();
        let json = extract_catalog_json(&bytes, anchor).expect("字符串里的括号不该干扰配平");
        let value: Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["models"][0]["description"], "a } brace {");
    }

    #[test]
    fn rejects_empty_or_malformed_catalog() {
        // 空 models:Codex 会拒("must contain at least one model"),我们也别写出去。
        let bytes = catalog_bytes(r#"{"models":[],"supported_reasoning_levels":1}"#);
        let anchor = find(&bytes, ANCHOR).unwrap();
        assert!(extract_catalog_json(&bytes, anchor).is_none());

        // 条目缺 slug。
        let bytes =
            catalog_bytes(r#"{"models":[{"supported_reasoning_levels":[{"effort":"low"}]}]}"#);
        let anchor = find(&bytes, ANCHOR).unwrap();
        assert!(extract_catalog_json(&bytes, anchor).is_none());
    }

    #[test]
    fn catalog_validation_rejects_empty_or_whitespace_slugs() {
        for slug in ["", "   "] {
            let catalog = serde_json::json!({
                "models": [{
                    "slug": slug,
                    "supported_reasoning_levels": [{"effort": "low"}],
                }],
            })
            .to_string();

            assert!(
                !is_valid_catalog_text(&catalog),
                "blank slug should be rejected: {slug:?}"
            );
        }
    }

    #[test]
    fn catalog_validation_requires_a_nonempty_reasoning_levels_array() {
        for model in [
            serde_json::json!({"slug": "gpt-5.6-sol"}),
            serde_json::json!({
                "slug": "gpt-5.6-sol",
                "supported_reasoning_levels": "low",
            }),
            serde_json::json!({
                "slug": "gpt-5.6-sol",
                "supported_reasoning_levels": [],
            }),
        ] {
            let catalog = serde_json::json!({"models": [model]}).to_string();
            assert!(
                !is_valid_catalog_text(&catalog),
                "invalid levels: {catalog}"
            );
        }
    }

    #[test]
    fn catalog_validation_rejects_levels_without_nonempty_string_effort() {
        for level in [
            serde_json::json!({}),
            serde_json::json!({"effort": 1}),
            serde_json::json!({"effort": ""}),
            serde_json::json!({"effort": "  "}),
        ] {
            let catalog = serde_json::json!({
                "models": [{
                    "slug": "gpt-5.6-sol",
                    "supported_reasoning_levels": [level],
                }],
            })
            .to_string();

            assert!(
                !is_valid_catalog_text(&catalog),
                "invalid effort should be rejected: {catalog}"
            );
        }
    }

    #[test]
    fn catalog_validation_accepts_an_existing_valid_catalog() {
        assert!(is_valid_catalog_text(ONE_MODEL));
    }

    /// 真机验证:在本机真实的 codex 二进制(几百 MB)里跑完整的分块扫描 + 提取。
    /// 标 `ignore` 是因为 CI / 没装 Codex 的机器上没有这个文件。手动跑:
    /// `cargo test -p quotio-core codex_catalog -- --ignored --nocapture`
    #[test]
    #[ignore = "需要本机装有 Codex CLI"]
    fn extracts_from_the_real_codex_binary() {
        let Some(binary) = locate_codex_cli() else {
            panic!("没找到 codex 二进制 —— locate_codex_cli 需要修");
        };
        println!("codex 二进制: {}", binary.display());
        let json = extract_from_binary(&binary).expect("应从真实二进制里提取出目录");
        let value: Value = serde_json::from_str(&json).expect("提取出来的必须是合法 JSON");
        let models = value["models"].as_array().expect("models 应为数组");
        println!("提取到 {} 个模型,{} 字节", models.len(), json.len());
        for m in models {
            let levels: Vec<&str> = m["supported_reasoning_levels"]
                .as_array()
                .unwrap()
                .iter()
                .map(|l| l["effort"].as_str().unwrap_or("?"))
                .collect();
            println!(
                "  {:22} {} 档: {}",
                m["slug"].as_str().unwrap_or("?"),
                levels.len(),
                levels.join(",")
            );
        }
        assert!(!models.is_empty());

        // 顺带验 reasoning_levels 这条真实路径:ensure_catalog(写盘/缓存) → 读回 → 按 slug 查。
        let sol = reasoning_levels("gpt-5.6-sol");
        println!("reasoning_levels(gpt-5.6-sol) = {sol:?}");
        assert!(
            sol.contains(&"ultra".to_string()),
            "sol 应支持 ultra,实得 {sol:?}"
        );
        assert!(sol.contains(&"max".to_string()), "sol 应支持 max");
        assert_eq!(reasoning_levels("gpt-5.5").len(), 4, "5.5 只有四档");
        assert!(
            reasoning_levels("不存在的模型").is_empty(),
            "未知模型应返回空,让前端回退"
        );
    }

    #[test]
    fn toml_path_uses_forward_slashes() {
        let p = PathBuf::from(r"C:\Users\x\.codex\quotio-model-catalog.json");
        assert_eq!(
            toml_path_value(&p),
            "C:/Users/x/.codex/quotio-model-catalog.json"
        );
        assert!(
            !toml_path_value(&p).contains('\\'),
            "反斜杠会触发 TOML 非法转义"
        );
    }
}
