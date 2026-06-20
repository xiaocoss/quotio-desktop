//! Manages the **kiro-rs** sidecar — a standalone Rust server that exposes Kiro
//! (AWS CodeWhisperer) as an Anthropic-compatible API on a local port.
//!
//! CLIProxyAPI (the proxy core Quotio drives) has no CodeWhisperer support, so
//! it cannot route to Kiro on its own. We run kiro-rs alongside it and register
//! the sidecar as a `claude-api-key` provider in `config.yaml`. That lets the
//! existing per-key→pool binding (provider id `kiro`) gate Kiro just like any
//! other pool: a client key bound to `kiro` is written as `allowed-api-keys` on
//! the sidecar provider, and requests for Kiro models flow
//! `client → CLIProxyAPI → kiro-rs → CodeWhisperer`.
//!
//! Quotio owns the sidecar's lifecycle: it derives kiro-rs credentials from the
//! existing `~/.cli-proxy-api/kiro-*.json` files, starts/stops the process with
//! the proxy, and only runs it when at least one Kiro account exists.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use serde_json::{json, Value};
use uuid::Uuid;

/// Host kiro-rs binds to (loopback only — it is an internal hop).
pub const KIRO_SIDECAR_HOST: &str = "127.0.0.1";
/// Port kiro-rs listens on. CLIProxyAPI defaults to 28317; we sit next to it.
pub const KIRO_SIDECAR_PORT: u16 = 28319;
/// Built-in provider id the API-key bindings use for Kiro.
pub const KIRO_PROVIDER_ID: &str = "kiro";

/// Directory holding kiro-rs's generated `config.json` + `credentials.json`.
fn work_dir() -> PathBuf {
    quotio_platform::app_config_dir().join("kiro-rs")
}

/// Stable local api-key the sidecar uses to authenticate its caller
/// (CLIProxyAPI). Persisted so the value written into kiro-rs's config matches
/// the one written into CLIProxyAPI's `config.yaml` across restarts. Disk-backed
/// (not held in the struct) so the config writer can read it without `&mut`.
pub fn current_api_key() -> String {
    let path = work_dir().join("api-key");
    if let Ok(existing) = fs::read_to_string(&path) {
        let trimmed = existing.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    let key = format!("sk-kiro-{}", Uuid::new_v4().simple());
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let _ = fs::write(&path, &key);
    key
}

/// Map one Quotio `kiro-*.json` (cockpit-tools layout) into a kiro-rs credential
/// object (camelCase, as kiro-rs expects). Returns `None` when the file lacks a
/// refresh token — kiro-rs needs it to mint access tokens.
fn map_credential(file: &Value) -> Option<Value> {
    let raw = file.get("kiro_auth_token_raw");
    let from_raw = |key: &str| -> Option<&str> {
        raw.and_then(|r| r.get(key))
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
    };

    let refresh_token = file
        .get("refresh_token")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .or_else(|| from_raw("refreshToken"))?
        .to_string();

    let mut cred = serde_json::Map::new();
    cred.insert("refreshToken".into(), json!(refresh_token));
    cred.insert(
        "authMethod".into(),
        json!(from_raw("authMethod").unwrap_or("social")),
    );

    if let Some(expires_at) = from_raw("expiresAt") {
        cred.insert("expiresAt".into(), json!(expires_at));
    }
    if let Some(email) = file
        .get("email")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
    {
        cred.insert("email".into(), json!(email));
    }

    // profileArn lives in kiro_auth_token_raw.profileArn or kiro_profile_raw.arn.
    let profile_arn = from_raw("profileArn").or_else(|| {
        file.get("kiro_profile_raw")
            .and_then(|p| p.get("arn"))
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
    });
    if let Some(arn) = profile_arn {
        cred.insert("profileArn".into(), json!(arn));
    }

    // Enterprise (IAM Identity Center) credentials, when present.
    for key in ["clientId", "clientSecret", "region"] {
        if let Some(value) = from_raw(key) {
            cred.insert(key.into(), json!(value));
        }
    }

    Some(Value::Object(cred))
}

/// Read every `kiro-*.json` under `auth_dir` and map them to kiro-rs credentials.
fn collect_credentials(auth_dir: &Path) -> Vec<Value> {
    let mut creds = Vec::new();
    let Ok(entries) = fs::read_dir(auth_dir) else {
        return creds;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_lowercase();
        if !name.starts_with("kiro-") || !name.ends_with(".json") {
            continue;
        }
        let Ok(text) = fs::read_to_string(entry.path()) else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        if let Some(cred) = map_credential(&value) {
            creds.push(cred);
        }
    }
    creds
}

/// Number of routable Kiro accounts found under `auth_dir`.
pub fn kiro_account_count(auth_dir: &Path) -> usize {
    collect_credentials(auth_dir).len()
}

/// Write kiro-rs's `config.json` + `credentials.json`. We deliberately do NOT set
/// `proxyUrl`: both Kiro endpoints (`kiro.dev` token refresh and `q.*.amazonaws`
/// runtime) are reachable directly here, and routing kiro-rs's reqwest client
/// through the upstream proxy actually BREAKS the refresh ("error sending
/// request"), which then disables the single Kiro credential. Direct works.
fn write_files(api_key: &str, creds: &[Value]) -> std::io::Result<()> {
    let dir = work_dir();
    fs::create_dir_all(&dir)?;

    let mut config = serde_json::Map::new();
    config.insert("host".into(), json!(KIRO_SIDECAR_HOST));
    config.insert("port".into(), json!(KIRO_SIDECAR_PORT));
    config.insert("apiKey".into(), json!(api_key));
    config.insert("region".into(), json!("us-east-1"));
    config.insert("defaultEndpoint".into(), json!("ide"));
    fs::write(
        dir.join("config.json"),
        serde_json::to_string_pretty(&Value::Object(config))?,
    )?;
    fs::write(
        dir.join("credentials.json"),
        serde_json::to_string_pretty(&Value::Array(creds.to_vec()))?,
    )?;
    Ok(())
}

/// Locate the kiro-rs binary, keeping the managed copy in sync with the bundled
/// one. The bundled resource (its OWN `resources/kiro/<platform>` dir — NEVER the
/// proxy resource dir, whose resolver would grab any `.exe` as the proxy core) is
/// the source of truth: it's (re)staged into the work dir whenever it differs by
/// size from the managed copy, so an app upgrade shipping a newer kiro-rs replaces
/// the old one instead of being shadowed by it. Falls back to an existing managed
/// copy when there's no bundle (a platform without the asset) or the copy fails
/// (binary in use). Mirrors the proxy core's `prepare_managed_binary`.
fn resolve_binary() -> Option<PathBuf> {
    let name = if cfg!(windows) { "kiro-rs.exe" } else { "kiro-rs" };
    let managed = work_dir().join(name);
    let bundled = quotio_platform::kiro_resource_dir().join(name);

    if bundled.is_file() {
        let stale = !managed.is_file() || file_len(&bundled) != file_len(&managed);
        if stale {
            let _ = fs::create_dir_all(work_dir());
            if fs::copy(&bundled, &managed).is_ok() {
                // Tauri's resource bundling can drop the unix exec bit; restore it
                // so the staged sidecar is launchable on macOS/Linux. No-op on Win.
                let _ = crate::make_executable(&managed);
                return Some(managed);
            }
            // Copy failed (e.g. managed locked by a running sidecar) — use whatever
            // copy already exists.
            return if managed.is_file() { Some(managed) } else { Some(bundled) };
        }
        return Some(managed);
    }

    if managed.is_file() {
        return Some(managed);
    }
    None
}

/// Byte size of `path`, or 0 if unreadable — cheap "did the binary change?" check.
fn file_len(path: &Path) -> u64 {
    fs::metadata(path).map(|meta| meta.len()).unwrap_or(0)
}

/// Single-quote a YAML scalar (doubling embedded quotes) — safe for keys/URLs.
fn yaml_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

/// Models advertised for the kiro-rs sidecar. CLIProxyAPI only routes a model to
/// a `claude-api-key` provider when that model is listed (empty ⇒ no routing). We
/// deliberately list only the set a Kiro FREE account can actually run — Sonnet
/// 4.5 and Haiku 4.5. Listing models the account can't use (Opus, Sonnet 4.6) is
/// a footgun: the upstream returns INVALID_MODEL_ID, which cools down the whole
/// Kiro auth and breaks the supported models too. Expand this for paid accounts.
const DEFAULT_KIRO_MODELS: &[&str] = &[
    "claude-sonnet-4-5-20250929",
    "claude-sonnet-4-5-20250929-thinking",
    "claude-haiku-4-5-20251001",
    "claude-haiku-4-5-20251001-thinking",
];

/// Render the `claude-api-key` provider block that routes to the sidecar.
/// `bound_keys` are client api-keys bound to the `kiro` pool; when present they
/// become `allowed-api-keys`, restricting the sidecar to those keys.
pub fn provider_yaml(api_key: &str, bound_keys: &[String]) -> String {
    let base_url = format!("http://{KIRO_SIDECAR_HOST}:{KIRO_SIDECAR_PORT}");
    let mut out = String::from("\n# Kiro (managed by Quotio — kiro-rs sidecar)\nclaude-api-key:\n");
    out.push_str(&format!("  - api-key: {}\n", yaml_quote(api_key)));
    out.push_str(&format!("    base-url: {}\n", yaml_quote(&base_url)));
    // A Kiro account is a single credential with no fallback, so CLIProxyAPI
    // cooldown only hurts: one transient/quota error parks the whole pool as
    // "no auth available". Disable cooling for this provider — retry in place.
    out.push_str("    disable-cooling: true\n");
    // Bare model names (no prefix): the quotio-key-router scheduler plugin routes
    // by inbound api-key, so Kiro must be a candidate under its real model ids.
    out.push_str("    models:\n");
    for model in DEFAULT_KIRO_MODELS {
        out.push_str(&format!("      - name: {}\n", yaml_quote(model)));
    }
    if !bound_keys.is_empty() {
        out.push_str("    allowed-api-keys:\n");
        for key in bound_keys {
            out.push_str(&format!("      - {}\n", yaml_quote(key)));
        }
    }
    out
}

/// Owns the kiro-rs child process. Lives inside the proxy lifecycle so the
/// sidecar starts/stops together with the core.
#[derive(Default)]
pub struct KiroSidecar {
    child: Option<Child>,
    active: bool,
}

impl KiroSidecar {
    /// Whether the sidecar is currently meant to be running (≥1 Kiro account).
    pub fn is_active(&self) -> bool {
        self.active
    }

    /// Refresh kiro-rs credentials from `auth_dir` and (re)start it when at least
    /// one Kiro account exists. Stops the sidecar when none remain.
    pub fn sync_and_start(&mut self, auth_dir: &Path) {
        let creds = collect_credentials(auth_dir);
        if creds.is_empty() {
            self.stop();
            self.active = false;
            return;
        }
        self.active = true;
        let _ = write_files(&current_api_key(), &creds);

        // Already running — the refreshed credential file is picked up on its own
        // reload; no need to bounce the process.
        if self.child.is_some() {
            return;
        }
        let Some(binary) = resolve_binary() else {
            return;
        };
        let dir = work_dir();
        let mut command = Command::new(&binary);
        command
            .arg("--config")
            .arg(dir.join("config.json"))
            .arg("--credentials")
            .arg(dir.join("credentials.json"))
            .current_dir(binary.parent().unwrap_or_else(|| Path::new(".")))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        }
        if let Ok(child) = command.spawn() {
            self.child = Some(child);
        }
    }

    /// Terminate the sidecar process if running.
    pub fn stop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_social_account_to_kiro_rs_credential() {
        // Mirrors the real cockpit-tools kiro-*.json layout.
        let file = json!({
            "refresh_token": "rt-top-level",
            "email": "user@example.com",
            "kiro_auth_token_raw": {
                "refreshToken": "rt-nested",
                "authMethod": "social",
                "expiresAt": "2026-06-18T17:07:40.000Z",
                "profileArn": "arn:aws:codewhisperer:us-east-1:123:profile/ABC"
            },
            "kiro_profile_raw": { "arn": "arn:aws:codewhisperer:us-east-1:123:profile/ABC" },
            "type": "kiro"
        });
        let cred = map_credential(&file).expect("should map");
        assert_eq!(cred["refreshToken"], json!("rt-top-level"));
        assert_eq!(cred["authMethod"], json!("social"));
        assert_eq!(cred["email"], json!("user@example.com"));
        assert_eq!(cred["expiresAt"], json!("2026-06-18T17:07:40.000Z"));
        assert_eq!(
            cred["profileArn"],
            json!("arn:aws:codewhisperer:us-east-1:123:profile/ABC")
        );
    }

    #[test]
    fn falls_back_to_nested_refresh_token_and_default_auth_method() {
        let file = json!({
            "kiro_auth_token_raw": { "refreshToken": "rt-nested" }
        });
        let cred = map_credential(&file).expect("should map from nested token");
        assert_eq!(cred["refreshToken"], json!("rt-nested"));
        assert_eq!(cred["authMethod"], json!("social"));
    }

    #[test]
    fn rejects_account_without_refresh_token() {
        let file = json!({ "email": "no-token@example.com" });
        assert!(map_credential(&file).is_none());
    }

    #[test]
    fn provider_yaml_includes_allowed_keys_when_bound() {
        let yaml = provider_yaml("sk-kiro-abc", &["sk-client-1".to_string()]);
        assert!(yaml.contains("claude-api-key:"));
        assert!(yaml.contains("base-url: 'http://127.0.0.1:28319'"));
        assert!(yaml.contains("allowed-api-keys:"));
        assert!(yaml.contains("- 'sk-client-1'"));
    }

    #[test]
    fn provider_yaml_omits_allowed_keys_when_unbound() {
        let yaml = provider_yaml("sk-kiro-abc", &[]);
        assert!(yaml.contains("api-key: 'sk-kiro-abc'"));
        assert!(!yaml.contains("allowed-api-keys"));
    }

    #[test]
    fn provider_yaml_declares_models_for_routing() {
        // CLIProxyAPI won't route to a claude-api-key provider with no models.
        let yaml = provider_yaml("sk-kiro-abc", &[]);
        assert!(yaml.contains("models:"));
        assert!(yaml.contains("- name: 'claude-sonnet-4-5-20250929'"));
        assert!(yaml.contains("- name: 'claude-haiku-4-5-20251001'"));
    }

}
