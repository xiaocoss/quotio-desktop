//! Cloudflared "quick tunnel" support, mirroring the macOS `CloudflaredService`.
//!
//! Downloads the cloudflared binary (user-initiated) and exposes the local
//! proxy through a public `https://*.trycloudflare.com` URL. The subprocess
//! lifecycle (spawn / read output / stop) is managed in the Tauri layer because
//! it needs the app handle to emit the detected URL to the UI; this module only
//! provides the download and the URL-extraction helper.

use std::fs;
use std::io::{Cursor, Read, Write};
use std::path::Path;
use std::time::Duration;

#[derive(Clone, Copy)]
enum CloudflaredFormat {
    Binary,
    Tgz,
}

struct CloudflaredAsset {
    url: &'static str,
    format: CloudflaredFormat,
}

/// Direct download URLs for the latest supported cloudflared builds.
const CLOUDFLARED_URL_WINDOWS: &str =
    "https://github.com/cloudflare/cloudflared/releases/latest/download/cloudflared-windows-amd64.exe";
const CLOUDFLARED_URL_LINUX_AMD64: &str =
    "https://github.com/cloudflare/cloudflared/releases/latest/download/cloudflared-linux-amd64";
const CLOUDFLARED_URL_LINUX_ARM64: &str =
    "https://github.com/cloudflare/cloudflared/releases/latest/download/cloudflared-linux-arm64";
const CLOUDFLARED_URL_DARWIN_AMD64: &str =
    "https://github.com/cloudflare/cloudflared/releases/latest/download/cloudflared-darwin-amd64.tgz";
const CLOUDFLARED_URL_DARWIN_ARM64: &str =
    "https://github.com/cloudflare/cloudflared/releases/latest/download/cloudflared-darwin-arm64.tgz";

fn cloudflared_asset() -> Result<CloudflaredAsset, String> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "x86_64") => Ok(CloudflaredAsset {
            url: CLOUDFLARED_URL_WINDOWS,
            format: CloudflaredFormat::Binary,
        }),
        ("linux", "x86_64") => Ok(CloudflaredAsset {
            url: CLOUDFLARED_URL_LINUX_AMD64,
            format: CloudflaredFormat::Binary,
        }),
        ("linux", "aarch64") => Ok(CloudflaredAsset {
            url: CLOUDFLARED_URL_LINUX_ARM64,
            format: CloudflaredFormat::Binary,
        }),
        ("macos", "x86_64") => Ok(CloudflaredAsset {
            url: CLOUDFLARED_URL_DARWIN_AMD64,
            format: CloudflaredFormat::Tgz,
        }),
        ("macos", "aarch64") => Ok(CloudflaredAsset {
            url: CLOUDFLARED_URL_DARWIN_ARM64,
            format: CloudflaredFormat::Tgz,
        }),
        (os, arch) => Err(format!("当前平台暂不支持下载 cloudflared：{}-{}", os, arch)),
    }
}

/// Download the cloudflared binary to `dest`. `on_progress(downloaded, total)`
/// is called during the transfer so the UI can render a percentage (total may
/// be 0 when the server omits Content-Length).
pub fn download_cloudflared(
    dest: &Path,
    proxy_url: Option<&str>,
    mut on_progress: impl FnMut(u64, u64),
) -> Result<(), String> {
    let agent = build_agent(proxy_url);
    let asset = cloudflared_asset()?;

    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|error| format!("创建隧道目录失败：{}", error))?;
    }

    let response = agent
        .get(asset.url)
        .set("User-Agent", "quotio-desktop")
        .call()
        .map_err(|error| format!("下载 cloudflared 失败：{}", error))?;

    let total = response
        .header("Content-Length")
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(0);

    let mut reader = response.into_reader();
    let tmp = dest.with_extension("download");
    let save_result = match asset.format {
        CloudflaredFormat::Binary => save_binary(&mut reader, total, &tmp, &mut on_progress),
        CloudflaredFormat::Tgz => read_download_bytes(&mut reader, total, &mut on_progress)
            .and_then(|bytes| unpack_cloudflared_tgz(&bytes, &tmp)),
    };
    if let Err(error) = save_result {
        let _ = fs::remove_file(&tmp);
        return Err(error);
    }

    let _ = fs::remove_file(dest);
    fs::rename(&tmp, dest).map_err(|error| {
        let _ = fs::remove_file(&tmp);
        format!("保存 cloudflared 失败：{}", error)
    })?;
    make_executable(dest)?;
    Ok(())
}

fn save_binary(
    reader: &mut impl Read,
    total: u64,
    tmp: &Path,
    on_progress: &mut impl FnMut(u64, u64),
) -> Result<(), String> {
    let mut file = fs::File::create(tmp).map_err(|error| format!("创建临时文件失败：{}", error))?;
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
    Ok(())
}

fn read_download_bytes(
    reader: &mut impl Read,
    total: u64,
    on_progress: &mut impl FnMut(u64, u64),
) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
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
        bytes.extend_from_slice(&buffer[..read]);
        downloaded += read as u64;
        on_progress(downloaded, total);
    }
    Ok(bytes)
}

fn unpack_cloudflared_tgz(bytes: &[u8], dest: &Path) -> Result<(), String> {
    let decoder = flate2::read::GzDecoder::new(Cursor::new(bytes));
    let mut archive = tar::Archive::new(decoder);
    let entries = archive
        .entries()
        .map_err(|error| format!("读取 cloudflared 压缩包失败：{}", error))?;

    for entry in entries {
        let mut entry = entry.map_err(|error| format!("读取 cloudflared 文件失败：{}", error))?;
        let path = entry
            .path()
            .map_err(|error| format!("读取 cloudflared 文件路径失败：{}", error))?;
        if path.file_name().and_then(|name| name.to_str()) == Some("cloudflared") {
            entry
                .unpack(dest)
                .map_err(|error| format!("解压 cloudflared 失败：{}", error))?;
            return Ok(());
        }
    }

    Err("cloudflared 压缩包中未找到可执行文件。".to_string())
}

#[cfg(unix)]
fn make_executable(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;

    let mut permissions = fs::metadata(path)
        .map_err(|error| format!("读取 cloudflared 权限失败：{}", error))?
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions)
        .map_err(|error| format!("设置 cloudflared 可执行权限失败：{}", error))
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) -> Result<(), String> {
    Ok(())
}

/// Extract the public quick-tunnel URL (`https://<sub>.trycloudflare.com`) from a
/// chunk of cloudflared log output, if present.
pub fn extract_tunnel_url(text: &str) -> Option<String> {
    const MARKER: &str = ".trycloudflare.com";
    let marker_idx = text.find(MARKER)?;
    let start = text[..marker_idx].rfind("https://")?;
    let end = marker_idx + MARKER.len();
    let candidate = &text[start..end];
    // Sanity: the subdomain between "https://" and the marker must be non-empty
    // and free of whitespace.
    let subdomain = &candidate["https://".len()..candidate.len() - MARKER.len()];
    if subdomain.is_empty() || subdomain.contains(char::is_whitespace) {
        return None;
    }
    Some(candidate.to_string())
}

fn build_agent(proxy_url: Option<&str>) -> ureq::Agent {
    let mut builder = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(15))
        .timeout_read(Duration::from_secs(180));
    // 优先用 App 配置的代理，回退系统环境变量代理（与 quota.rs / proxy_download 一致）。
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

#[cfg(test)]
mod tests {
    use super::extract_tunnel_url;

    #[test]
    fn extracts_quick_tunnel_url() {
        let line = "2024-01-01 INF +-----+ | https://happy-tree-1234.trycloudflare.com | +-----+";
        assert_eq!(
            extract_tunnel_url(line).as_deref(),
            Some("https://happy-tree-1234.trycloudflare.com")
        );
    }

    #[test]
    fn ignores_lines_without_url() {
        assert_eq!(extract_tunnel_url("INF Starting tunnel"), None);
    }
}
