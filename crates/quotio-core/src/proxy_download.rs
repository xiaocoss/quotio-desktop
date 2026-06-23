//! Downloads the CLIProxyAPI core binary from its official GitHub releases,
//! mirroring what the original macOS app does (it bundles darwin builds and
//! downloads the rest). The release publishes per-platform archives; we pick
//! the one matching the current OS/arch, download it, and extract the binary.
//!
//! The actual download is user-initiated (the user clicks "download proxy core"
//! in the app), and the source is the well-known upstream repo this app wraps.

use std::fs;
use std::io::{copy, Read, Write};
use std::path::Path;
use std::time::Duration;

use serde::Deserialize;

const RELEASE_URL: &str = "https://api.github.com/repos/router-for-me/CLIProxyAPI/releases/latest";

#[derive(Debug, Deserialize)]
struct Release {
    #[serde(default)]
    tag_name: Option<String>,
    #[serde(default)]
    assets: Vec<Asset>,
}

#[derive(Debug, Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
    #[serde(default)]
    size: u64,
}

/// Download + extract the proxy binary to `dest`. Returns the release tag.
/// `on_progress(downloaded_bytes, total_bytes)` is called during the download
/// so the UI can show a percentage (total may be 0 if unknown).
pub fn download_proxy_binary(
    dest: &Path,
    proxy_url: Option<&str>,
    mut on_progress: impl FnMut(u64, u64),
) -> Result<String, String> {
    let agent = build_agent(proxy_url);

    let release: Release = agent
        .get(RELEASE_URL)
        .set("User-Agent", "quotio-desktop")
        .set("Accept", "application/vnd.github+json")
        .call()
        .map_err(|error| format!("获取 CLIProxyAPI release 失败：{}", error))?
        .into_json()
        .map_err(|error| format!("解析 release 信息失败：{}", error))?;

    let asset = release
        .assets
        .iter()
        .find(|asset| asset_matches_platform(&asset.name))
        .ok_or_else(|| "在最新 release 中未找到当前平台(OS/架构)的代理二进制资产。".to_string())?;

    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|error| format!("创建代理目录失败：{}", error))?;
    }

    let archive_path = std::env::temp_dir().join(format!("quotio-{}", asset.name));
    {
        let response = agent
            .get(&asset.browser_download_url)
            .set("User-Agent", "quotio-desktop")
            .call()
            .map_err(|error| format!("下载代理二进制失败：{}", error))?;
        let total = response
            .header("Content-Length")
            .and_then(|value| value.parse::<u64>().ok())
            .filter(|value| *value > 0)
            .unwrap_or(asset.size);
        let mut reader = response.into_reader();
        let mut file = fs::File::create(&archive_path)
            .map_err(|error| format!("创建临时文件失败：{}", error))?;
        let mut buffer = [0u8; 65536];
        let mut downloaded: u64 = 0;
        on_progress(0, total);
        loop {
            let read = reader
                .read(&mut buffer)
                .map_err(|error| format!("读取下载流失败：{}", error))?;
            if read == 0 {
                break;
            }
            file.write_all(&buffer[..read])
                .map_err(|error| format!("写入下载内容失败：{}", error))?;
            downloaded += read as u64;
            on_progress(downloaded, total);
        }
    }

    let result = extract_binary(&archive_path, dest, &asset.name);
    let _ = fs::remove_file(&archive_path);
    result?;

    Ok(release.tag_name.unwrap_or_default())
}

fn extract_binary(archive: &Path, dest: &Path, asset_name: &str) -> Result<(), String> {
    let lower = asset_name.to_lowercase();
    if lower.ends_with(".zip") {
        let result = extract_from_zip(archive, dest);
        #[cfg(unix)]
        make_executable(dest);
        result
    } else if lower.ends_with(".tar.gz") || lower.ends_with(".tgz") {
        let result = extract_from_tar_gz(archive, dest);
        #[cfg(unix)]
        make_executable(dest);
        result
    } else {
        Err(format!(
            "暂不支持自动解包该资产格式（{}）。目前支持 .zip 和 .tar.gz。",
            asset_name
        ))
    }
}

#[cfg(unix)]
fn make_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    if let Ok(mut perms) = fs::metadata(path).map(|m| m.permissions()) {
        perms.set_mode(0o755);
        let _ = fs::set_permissions(path, perms);
    }
}

fn extract_from_zip(archive: &Path, dest: &Path) -> Result<(), String> {
    let file = fs::File::open(archive).map_err(|error| format!("打开压缩包失败：{}", error))?;
    let mut zip =
        zip::ZipArchive::new(file).map_err(|error| format!("读取 zip 失败：{}", error))?;

    let mut target_index: Option<usize> = None;
    for index in 0..zip.len() {
        let entry = zip
            .by_index(index)
            .map_err(|error| format!("读取 zip 条目失败：{}", error))?;
        if !entry.is_file() {
            continue;
        }
        let name = entry.name().to_lowercase();
        let base = name.rsplit(['/', '\\']).next().unwrap_or(&name);
        let looks_like_binary = base.ends_with(".exe")
            || base == "cliproxyapi"
            || base == "cli-proxy-api"
            || base.starts_with("cliproxyapi");
        if looks_like_binary {
            target_index = Some(index);
            break;
        }
    }

    let index = target_index.ok_or_else(|| "压缩包内未找到代理可执行文件。".to_string())?;
    let mut entry = zip
        .by_index(index)
        .map_err(|error| format!("提取 zip 条目失败：{}", error))?;
    let mut out = fs::File::create(dest).map_err(|error| format!("写入二进制失败：{}", error))?;
    copy(&mut entry, &mut out).map_err(|error| format!("解包二进制失败：{}", error))?;
    Ok(())
}

fn extract_from_tar_gz(archive: &Path, dest: &Path) -> Result<(), String> {
    let file = fs::File::open(archive).map_err(|error| format!("打开压缩包失败：{}", error))?;
    let tar = flate2::read::GzDecoder::new(file);
    let mut archive = tar::Archive::new(tar);

    for entry in archive.entries().map_err(|error| format!("读取 tar 失败：{}", error))? {
        let mut entry = entry.map_err(|error| format!("读取 tar 条目失败：{}", error))?;
        if entry.header().entry_type() != tar::EntryType::Regular {
            continue;
        }

        let path = entry.path().map_err(|error| format!("获取条目路径失败：{}", error))?;
        let name = path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_lowercase();
        
        let looks_like_binary = name == "cliproxyapi"
            || name == "cli-proxy-api"
            || name.starts_with("cliproxyapi");

        if looks_like_binary {
            let mut out = fs::File::create(dest).map_err(|error| format!("写入二进制失败：{}", error))?;
            copy(&mut entry, &mut out).map_err(|error| format!("解包二进制失败：{}", error))?;
            return Ok(());
        }
    }

    Err("压缩包内未找到代理可执行文件。".to_string())
}

/// Match a release asset name to the current platform (OS + architecture).
fn asset_matches_platform(name: &str) -> bool {
    let lower = name.to_lowercase();
    if lower.contains("checksum") || lower.ends_with(".sha256") || lower.ends_with(".sig") {
        return false;
    }

    let os_ok = if cfg!(target_os = "windows") {
        lower.contains("windows")
    } else if cfg!(target_os = "macos") {
        lower.contains("darwin") || lower.contains("macos")
    } else {
        lower.contains("linux")
    };

    let arch_ok = if cfg!(target_arch = "x86_64") {
        lower.contains("amd64") || lower.contains("x86_64") || lower.contains("x64")
    } else if cfg!(target_arch = "aarch64") {
        lower.contains("arm64") || lower.contains("aarch64")
    } else {
        true
    };

    let ext_ok = lower.ends_with(".zip") || lower.ends_with(".tar.gz") || lower.ends_with(".tgz");

    os_ok && arch_ok && ext_ok
}

fn build_agent(proxy_url: Option<&str>) -> ureq::Agent {
    let mut builder = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(15))
        .timeout_read(Duration::from_secs(180));
    // 优先用 App 里配置的代理，回退系统环境变量代理（与 quota.rs 一致），
    // 否则国内直连 GitHub 下不动核心。
    let chosen = proxy_url
        .map(str::trim)
        .filter(|url| !url.is_empty())
        .map(ToOwned::to_owned)
        .or_else(proxy_from_env);
    if let Some(url) = chosen {
        if let Ok(proxy) = ureq::Proxy::new(&url) {
            builder = builder.proxy(proxy);
        }
    }
    builder.build()
}

fn proxy_from_env() -> Option<String> {
    for key in [
        "HTTPS_PROXY",
        "https_proxy",
        "HTTP_PROXY",
        "http_proxy",
        "ALL_PROXY",
        "all_proxy",
    ] {
        if let Ok(value) = std::env::var(key) {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    None
}
