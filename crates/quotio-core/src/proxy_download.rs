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
use sha2::{Digest, Sha256};

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

    // 先取该资产的期望 SHA256(从 release 发布的 checksums 文件解析),供下载后比对。
    let expected_sha = find_expected_sha256(&release, &agent, &asset.name);

    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|error| format!("创建代理目录失败：{}", error))?;
    }

    let archive_path = std::env::temp_dir().join(format!("quotio-{}", asset.name));
    let mut hasher = Sha256::new();
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
            hasher.update(&buffer[..read]);
            downloaded += read as u64;
            on_progress(downloaded, total);
        }
    }

    // 完整性校验:与 release 发布的校验和比对。若校验和缺失则记录警告并继续(避免
    // 上游改格式后下载彻底失败);匹配失败则中止,绝不解包/执行被篡改的二进制。
    let actual_sha = format!("{:x}", hasher.finalize());
    match &expected_sha {
        Some(expected) if !expected.eq_ignore_ascii_case(&actual_sha) => {
            let _ = fs::remove_file(&archive_path);
            return Err(format!(
                "代理二进制校验和不匹配(期望 {expected},实际 {actual_sha}),已中止以防执行被篡改的二进制。"
            ));
        }
        Some(_) => {}
        None => {
            eprintln!(
                "[proxy_download] 警告:release 未提供可匹配的校验和,跳过完整性校验({})",
                asset.name
            );
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
        extract_from_zip(archive, dest)
    } else {
        Err(format!(
            "暂不支持自动解包该资产格式（{}）。目前支持 Windows 的 .zip。",
            asset_name
        ))
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

/// 从 release 里找到适用于 `asset_name` 的 SHA256:优先整包 `*checksums*` 文件,
/// 其次针对该资产的 `<name>.sha256`。解析 `<hash>  <filename>` 行,按 basename 匹配。
/// 取不到(无校验和资产 / 下载失败 / 无匹配行)返回 None,由调用方决定降级处理。
fn find_expected_sha256(release: &Release, agent: &ureq::Agent, asset_name: &str) -> Option<String> {
    let want_base = asset_name
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(asset_name);
    let per_asset = format!("{}.sha256", asset_name).to_lowercase();
    let checksum_asset = release.assets.iter().find(|a| {
        let lower = a.name.to_lowercase();
        lower.contains("checksum") || lower == per_asset
    })?;
    let is_per_asset = checksum_asset.name.to_lowercase() == per_asset;

    let body = agent
        .get(&checksum_asset.browser_download_url)
        .set("User-Agent", "quotio-desktop")
        .call()
        .ok()?
        .into_string()
        .ok()?;

    // 针对单个资产的 `<name>.sha256`:内容通常就是该资产的哈希(可能后跟文件名),
    // 直接取第一个 token。
    if is_per_asset {
        return body
            .split_whitespace()
            .next()
            .filter(|value| !value.is_empty())
            .map(|value| value.to_lowercase());
    }

    for line in body.lines() {
        let mut parts = line.split_whitespace();
        let hash = match parts.next() {
            Some(value) if !value.is_empty() => value,
            _ => continue,
        };
        // 文件名取该行最后一个字段;部分格式用 `*name` 标二进制模式,去掉前缀星号。
        let file = match parts.last() {
            Some(value) => value.trim_start_matches('*'),
            None => continue,
        };
        let file_base = file.rsplit(['/', '\\']).next().unwrap_or(file);
        if file_base.eq_ignore_ascii_case(want_base) {
            return Some(hash.to_lowercase());
        }
    }
    None
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
