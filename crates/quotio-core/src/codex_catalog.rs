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

use serde_json::Value;
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// 目录 JSON 的落盘位置(Codex 的家目录里,和 config.toml 同级)。
fn catalog_path() -> PathBuf {
    quotio_platform::expand_home_path("~/.codex/quotio-model-catalog.json")
}

/// 记录「这份目录是从哪个二进制、什么指纹提取的」,用来判断要不要重提。
fn meta_path() -> PathBuf {
    quotio_platform::expand_home_path("~/.codex/quotio-model-catalog.meta.json")
}

/// 目录里必定出现、且不随模型增减而变的锚点键。
const ANCHOR: &[u8] = b"\"supported_reasoning_levels\"";
/// 锚点往前找 `"models"` 的最大回溯距离(单个模型条目的前半段不会比这更长)。
const BACK_WINDOW: usize = 16 * 1024;
/// 从目录起始位置往后取多大一块来做花括号配平。目录实测约 300KB,留足余量。
const FORWARD_WINDOW: usize = 8 * 1024 * 1024;

/// 确保目录文件存在且与当前 codex 二进制同步,返回它的路径。
/// 任何一步失败都返回 `None` —— 调用方据此**跳过** `model_catalog_json`,保持旧行为。
pub fn ensure_catalog() -> Option<PathBuf> {
    let binary = locate_codex_cli()?;
    let fingerprint = fingerprint(&binary)?;
    let target = catalog_path();

    if target.is_file() && fs::read_to_string(meta_path()).ok().as_deref() == Some(&fingerprint) {
        return Some(target);
    }

    let json = extract_from_binary(&binary)?;
    if let Some(parent) = target.parent() {
        let _ = fs::create_dir_all(parent);
    }
    fs::write(&target, json).ok()?;
    // meta 写失败不致命:下次会重提一遍,只是白花点时间。
    let _ = fs::write(meta_path(), &fingerprint);
    Some(target)
}

/// 二进制指纹:路径 + 大小 + mtime。Codex 升级后三者必有变化 → 触发重新提取。
fn fingerprint(binary: &Path) -> Option<String> {
    let meta = fs::metadata(binary).ok()?;
    let mtime = meta
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs();
    Some(format!("{}|{}|{}", binary.display(), meta.len(), mtime))
}

/// 定位 Codex CLI 二进制。桌面应用是靠它跑的(config.toml 里的 `CODEX_CLI_PATH` 即指向它),
/// 所以它内置的目录就是 Codex 实际使用的那份。
fn locate_codex_cli() -> Option<PathBuf> {
    let exe = if cfg!(windows) { "codex.exe" } else { "codex" };

    let mut roots: Vec<PathBuf> = Vec::new();
    #[cfg(windows)]
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        roots.push(PathBuf::from(local).join("OpenAI").join("Codex").join("bin"));
    }
    #[cfg(target_os = "macos")]
    roots.push(quotio_platform::expand_home_path(
        "~/Library/Application Support/OpenAI/Codex/bin",
    ));
    #[cfg(target_os = "linux")]
    roots.push(quotio_platform::expand_home_path("~/.local/share/OpenAI/Codex/bin"));
    roots.push(quotio_platform::expand_home_path("~/.codex/bin"));

    for root in &roots {
        // 直接放在 bin/ 下。
        let direct = root.join(exe);
        if direct.is_file() {
            return Some(direct);
        }
        // 实际布局是 bin/<hash>/codex.exe,可能并存多个版本 —— 取 mtime 最新的。
        let mut best: Option<(std::time::SystemTime, PathBuf)> = None;
        if let Ok(entries) = fs::read_dir(root) {
            for entry in entries.flatten() {
                let candidate = entry.path().join(exe);
                if !candidate.is_file() {
                    continue;
                }
                let Ok(modified) = fs::metadata(&candidate).and_then(|m| m.modified()) else {
                    continue;
                };
                if best.as_ref().is_none_or(|(t, _)| modified > *t) {
                    best = Some((modified, candidate));
                }
            }
        }
        if let Some((_, path)) = best {
            return Some(path);
        }
    }
    None
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
        if n < overlap {
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

    let value: Value = serde_json::from_slice(raw).ok()?;
    let models = value.get("models")?.as_array()?;
    // 空目录会被 Codex 拒:"must contain at least one model"。字段不对也别写出去。
    if models.is_empty()
        || !models.iter().all(|m| {
            m.get("slug").and_then(Value::as_str).is_some()
                && m.get("supported_reasoning_levels")
                    .and_then(Value::as_array)
                    .is_some_and(|levels| !levels.is_empty())
        })
    {
        return None;
    }
    String::from_utf8(raw.to_vec()).ok()
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
    let slug = model_slug.trim();
    if slug.is_empty() {
        return Vec::new();
    }
    let Some(path) = ensure_catalog() else {
        return Vec::new();
    };
    let Ok(text) = fs::read_to_string(path) else {
        return Vec::new();
    };
    let Ok(value) = serde_json::from_str::<Value>(&text) else {
        return Vec::new();
    };
    value
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
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn catalog_bytes(inner: &str) -> Vec<u8> {
        format!("....junk....{inner}....trailing junk....").into_bytes()
    }

    const ONE_MODEL: &str = r#"{"models":[{"slug":"gpt-5.6-sol","supported_reasoning_levels":[{"effort":"low"},{"effort":"ultra"}]}]}"#;

    #[test]
    fn extracts_catalog_around_the_anchor() {
        let bytes = catalog_bytes(ONE_MODEL);
        let anchor = find(&bytes, ANCHOR).expect("锚点应存在");
        let json = extract_catalog_json(&bytes, anchor).expect("应提取出目录");
        let value: Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["models"][0]["slug"], "gpt-5.6-sol");
        assert_eq!(value["models"][0]["supported_reasoning_levels"][1]["effort"], "ultra");
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
        let bytes = catalog_bytes(r#"{"models":[{"supported_reasoning_levels":[{"effort":"low"}]}]}"#);
        let anchor = find(&bytes, ANCHOR).unwrap();
        assert!(extract_catalog_json(&bytes, anchor).is_none());
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
            println!("  {:22} {} 档: {}", m["slug"].as_str().unwrap_or("?"), levels.len(), levels.join(","));
        }
        assert!(!models.is_empty());

        // 顺带验 reasoning_levels 这条真实路径:ensure_catalog(写盘/缓存) → 读回 → 按 slug 查。
        let sol = reasoning_levels("gpt-5.6-sol");
        println!("reasoning_levels(gpt-5.6-sol) = {sol:?}");
        assert!(sol.contains(&"ultra".to_string()), "sol 应支持 ultra,实得 {sol:?}");
        assert!(sol.contains(&"max".to_string()), "sol 应支持 max");
        assert_eq!(reasoning_levels("gpt-5.5").len(), 4, "5.5 只有四档");
        assert!(reasoning_levels("不存在的模型").is_empty(), "未知模型应返回空,让前端回退");
    }

    #[test]
    fn toml_path_uses_forward_slashes() {
        let p = PathBuf::from(r"C:\Users\x\.codex\quotio-model-catalog.json");
        assert_eq!(
            toml_path_value(&p),
            "C:/Users/x/.codex/quotio-model-catalog.json"
        );
        assert!(!toml_path_value(&p).contains('\\'), "反斜杠会触发 TOML 非法转义");
    }
}
