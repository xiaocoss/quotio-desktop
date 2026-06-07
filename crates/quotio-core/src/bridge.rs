//! ProxyBridge — a small HTTP-aware reverse proxy that sits between CLI tools
//! and CLIProxyAPI to provide model *fallback*.
//!
//! When a request targets a virtual model (configured on the Fallback page),
//! the bridge resolves it to a prioritized list of real models and retries down
//! the list when an attempt fails (e.g. quota exhausted / 429 / 5xx). Requests
//! that don't reference a virtual model are forwarded transparently. The bridge
//! forces `Connection: close` on every upstream request to keep connections
//! fresh (the original stale-connection fix) and streams successful responses
//! straight back to the client so SSE/streaming keeps working.
//!
//! The fallback configuration is read from the persisted config file on each
//! request, so it always reflects the latest UI state without a restart.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use quotio_types::{FallbackConfiguration, FallbackEntry};

pub struct ProxyBridge {
    shutdown: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
    listen_port: u16,
}

impl ProxyBridge {
    /// Start the bridge on `listen_port`, forwarding to CLIProxyAPI on
    /// `target_port`. `fallback_path` points at the persisted fallback config.
    pub fn start(
        listen_port: u16,
        target_port: u16,
        fallback_path: PathBuf,
    ) -> Result<Self, String> {
        let listener = TcpListener::bind(("127.0.0.1", listen_port))
            .map_err(|error| format!("绑定桥接端口 {} 失败：{}", listen_port, error))?;
        listener
            .set_nonblocking(true)
            .map_err(|error| format!("配置桥接监听失败：{}", error))?;

        let shutdown = Arc::new(AtomicBool::new(false));
        let shutdown_loop = shutdown.clone();
        let handle = std::thread::spawn(move || {
            for stream in listener.incoming() {
                if shutdown_loop.load(Ordering::Relaxed) {
                    break;
                }
                match stream {
                    Ok(client) => {
                        let path = fallback_path.clone();
                        std::thread::spawn(move || {
                            let _ = handle_connection(client, target_port, &path);
                        });
                    }
                    Err(ref error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(40));
                    }
                    Err(_) => break,
                }
            }
        });

        Ok(ProxyBridge {
            shutdown,
            handle: Some(handle),
            listen_port,
        })
    }

    pub fn stop(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
        // Nudge the accept loop so it observes the shutdown flag promptly.
        let _ = TcpStream::connect(("127.0.0.1", self.listen_port));
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for ProxyBridge {
    fn drop(&mut self) {
        self.stop();
    }
}

fn handle_connection(
    client: TcpStream,
    target_port: u16,
    fallback_path: &Path,
) -> std::io::Result<()> {
    client.set_read_timeout(Some(Duration::from_secs(300)))?;
    let mut write_client = client.try_clone()?;
    let (req_head, req_body) = read_message(&client)?;
    if req_head.is_empty() {
        return Ok(());
    }

    let model = extract_model(&req_body);
    let chain = resolve_chain(model.as_deref(), fallback_path);

    // Either a fallback chain (virtual model) or a single transparent pass-through.
    let candidates: Vec<Option<String>> = if chain.is_empty() {
        vec![None]
    } else {
        chain.into_iter().map(Some).collect()
    };

    for (index, candidate) in candidates.iter().enumerate() {
        let body = match candidate {
            Some(model) => rewrite_model(&req_body, model),
            None => req_body.clone(),
        };
        let head = rebuild_head(&req_head, body.len());

        let upstream = match TcpStream::connect(("127.0.0.1", target_port)) {
            Ok(stream) => stream,
            Err(_) => continue,
        };
        upstream.set_read_timeout(Some(Duration::from_secs(300)))?;
        let mut write_upstream = upstream.try_clone()?;
        write_upstream.write_all(head.as_bytes())?;
        write_upstream.write_all(&body)?;
        write_upstream.flush()?;

        let mut reader = BufReader::new(upstream);
        let (status, resp_head) = read_response_head(&mut reader)?;
        let is_last = index + 1 == candidates.len();

        if (200..300).contains(&status) || is_last {
            // Success (or out of fallbacks): relay headers, then stream the body.
            write_client.write_all(resp_head.as_bytes())?;
            std::io::copy(&mut reader, &mut write_client)?;
            write_client.flush()?;
            return Ok(());
        }

        // Failed attempt with candidates remaining: drain the body and retry.
        let mut sink = Vec::new();
        let _ = reader.read_to_end(&mut sink);
    }

    Ok(())
}

/// Read an HTTP message head (up to the blank line) plus a Content-Length body.
fn read_message(stream: &TcpStream) -> std::io::Result<(String, Vec<u8>)> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut head = String::new();
    loop {
        let mut line = String::new();
        let read = reader.read_line(&mut line)?;
        if read == 0 {
            break;
        }
        head.push_str(&line);
        if line == "\r\n" || line == "\n" {
            break;
        }
    }
    let length = content_length(&head);
    let mut body = vec![0u8; length];
    if length > 0 {
        reader.read_exact(&mut body)?;
    }
    Ok((head, body))
}

fn read_response_head(reader: &mut BufReader<TcpStream>) -> std::io::Result<(u16, String)> {
    let mut head = String::new();
    loop {
        let mut line = String::new();
        let read = reader.read_line(&mut line)?;
        if read == 0 {
            break;
        }
        head.push_str(&line);
        if line == "\r\n" || line == "\n" {
            break;
        }
    }
    let status = head
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse::<u16>().ok())
        .unwrap_or(502);
    Ok((status, head))
}

fn content_length(head: &str) -> usize {
    for line in head.split("\r\n") {
        if line.to_ascii_lowercase().starts_with("content-length:") {
            if let Some(rest) = line.splitn(2, ':').nth(1) {
                if let Ok(value) = rest.trim().parse::<usize>() {
                    return value;
                }
            }
        }
    }
    0
}

/// Rebuild the request head with a new Content-Length and forced Connection: close.
fn rebuild_head(head: &str, content_length: usize) -> String {
    let mut lines = head.split("\r\n");
    let request_line = lines.next().unwrap_or("");
    let mut out = String::new();
    out.push_str(request_line);
    out.push_str("\r\n");
    for line in lines {
        if line.is_empty() {
            continue;
        }
        let lower = line.to_ascii_lowercase();
        if lower.starts_with("content-length:")
            || lower.starts_with("connection:")
            || lower.starts_with("transfer-encoding:")
        {
            continue;
        }
        out.push_str(line);
        out.push_str("\r\n");
    }
    out.push_str(&format!("Content-Length: {}\r\n", content_length));
    out.push_str("Connection: close\r\n\r\n");
    out
}

fn extract_model(body: &[u8]) -> Option<String> {
    let value: serde_json::Value = serde_json::from_slice(body).ok()?;
    value.get("model")?.as_str().map(|model| model.to_string())
}

fn rewrite_model(body: &[u8], model: &str) -> Vec<u8> {
    if let Ok(mut value) = serde_json::from_slice::<serde_json::Value>(body) {
        if let Some(object) = value.as_object_mut() {
            object.insert(
                "model".to_string(),
                serde_json::Value::String(model.to_string()),
            );
            if let Ok(bytes) = serde_json::to_vec(&value) {
                return bytes;
            }
        }
    }
    body.to_vec()
}

/// Resolve a request model to its fallback chain (prioritized real model ids).
/// Returns empty when fallback is disabled or the model isn't a virtual model.
fn resolve_chain(model: Option<&str>, fallback_path: &Path) -> Vec<String> {
    let Some(model) = model else {
        return Vec::new();
    };
    let Ok(raw) = std::fs::read_to_string(fallback_path) else {
        return Vec::new();
    };
    let Ok(config) = serde_json::from_str::<FallbackConfiguration>(&raw) else {
        return Vec::new();
    };
    if !config.is_enabled {
        return Vec::new();
    }
    let Some(virtual_model) = config
        .virtual_models
        .iter()
        .find(|candidate| candidate.name == model && candidate.is_enabled)
    else {
        return Vec::new();
    };
    let mut entries: Vec<&FallbackEntry> = virtual_model.fallback_entries.iter().collect();
    entries.sort_by_key(|entry| entry.priority);
    entries
        .into_iter()
        .map(|entry| entry.model_id.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{content_length, extract_model, rebuild_head, rewrite_model};

    #[test]
    fn parses_content_length() {
        assert_eq!(
            content_length("POST / HTTP/1.1\r\nContent-Length: 42\r\n\r\n"),
            42
        );
        assert_eq!(content_length("GET / HTTP/1.1\r\n\r\n"), 0);
    }

    #[test]
    fn extracts_and_rewrites_model() {
        let body = br#"{"model":"quotio-x","stream":true}"#;
        assert_eq!(extract_model(body).as_deref(), Some("quotio-x"));
        let rewritten = rewrite_model(body, "gpt-5");
        assert_eq!(extract_model(&rewritten).as_deref(), Some("gpt-5"));
    }

    #[test]
    fn rebuild_head_forces_connection_close() {
        let head =
            "POST /v1/chat HTTP/1.1\r\nHost: x\r\nContent-Length: 5\r\nConnection: keep-alive\r\n\r\n";
        let out = rebuild_head(head, 10);
        assert!(out.contains("Connection: close"));
        assert!(out.contains("Content-Length: 10"));
        assert!(!out.to_ascii_lowercase().contains("keep-alive"));
    }
}
