//! Cloudflared "quick tunnel" support, mirroring the macOS `CloudflaredService`.
//!
//! Downloads the cloudflared binary (user-initiated) and exposes the local
//! proxy through a public `https://*.trycloudflare.com` URL. The subprocess
//! lifecycle (spawn / read output / stop) is managed in the Tauri layer because
//! it needs the app handle to emit the detected URL to the UI; this module only
//! provides the download and the URL-extraction helper.

use std::fs;
use std::io::{Read, Write};
use std::path::Path;
use std::time::Duration;

/// Direct download URL for the latest Windows amd64 cloudflared build.
const CLOUDFLARED_URL_WINDOWS: &str =
    "https://github.com/cloudflare/cloudflared/releases/latest/download/cloudflared-windows-amd64.exe";

fn download_url() -> &'static str {
    // This build targets Windows amd64; other platforms would extend this.
    CLOUDFLARED_URL_WINDOWS
}

/// Download the cloudflared binary to `dest`. `on_progress(downloaded, total)`
/// is called during the transfer so the UI can render a percentage (total may
/// be 0 when the server omits Content-Length).
pub fn download_cloudflared(dest: &Path, mut on_progress: impl FnMut(u64, u64)) -> Result<(), String> {
    let agent = build_agent();

    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|error| format!("创建隧道目录失败：{}", error))?;
    }

    let response = agent
        .get(download_url())
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
    {
        let mut file =
            fs::File::create(&tmp).map_err(|error| format!("创建临时文件失败：{}", error))?;
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

    fs::rename(&tmp, dest).map_err(|error| {
        let _ = fs::remove_file(&tmp);
        format!("保存 cloudflared 失败：{}", error)
    })?;
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

fn build_agent() -> ureq::Agent {
    let mut builder = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(15))
        .timeout_read(Duration::from_secs(180));
    if let Some(url) = proxy_from_env() {
        if let Ok(proxy) = ureq::Proxy::new(&url) {
            builder = builder.proxy(proxy);
        }
    }
    builder.build()
}

fn proxy_from_env() -> Option<String> {
    for key in ["HTTPS_PROXY", "https_proxy", "HTTP_PROXY", "http_proxy", "ALL_PROXY", "all_proxy"] {
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
