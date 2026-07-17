# Codex Login-Only Session Recovery Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Recover only committed Quotio-managed Codex sessions across app restarts, roll back incomplete launches, and reliably remove stale `quotio_bound_login_only` markers without losing retryable cleanup identity.

**Architecture:** Extend `~/.codex/quotio-launch-backup.json` with optional non-secret session metadata and a `Prepared`/`Running` commit phase. Startup immediately rolls back `Prepared`, recovers identity but never the persisted pid from `Running`, and records failed cleanup in `AppCore::codex_cleanup_pending`. Centralized disk cleanup restores account markers and Codex files independently, while the shared atomic writer provides durable replacement and temp-file cleanup on Windows.

**Tech Stack:** Rust, serde/serde_json, `windows-sys` `MoveFileExW`, Quotio atomic file writes, Cargo tests, TypeScript/Vite build, Tauri release workflow, GitHub CLI.

---

### Task 0: Remove unrelated rose-theme commits from the release branch

**Files:**
- Verify only: `docs/superpowers/specs/2026-07-16-rose-floral-theme-design.md`
- Verify only: `docs/superpowers/plans/2026-07-16-rose-floral-theme.md`

- [ ] **Step 1: Confirm the release branch currently contains the two unrelated commits**

Run:

```powershell
git log --oneline origin/main..HEAD
```

Expected: `b03d020` and `7370bc9` appear between the PowerShell planning commits and the PowerShell implementation commits.

- [ ] **Step 2: Rebase the release-only commits onto the PowerShell plan commit**

Run:

```powershell
git rebase --onto 2192ccb 7370bc9 codex/powershell-v0.7.1
```

Expected: the PowerShell implementation, login-only design, and this plan are replayed; the two rose-theme commits are no longer ancestors of the release branch.

- [ ] **Step 3: Verify release scope**

Run:

```powershell
git log --oneline origin/main..HEAD
git diff --name-only origin/main...HEAD
```

Expected: no rose-theme spec, plan, assets, CSS, or component files appear.

### Task 1: Persist Codex launch-session metadata in the launch backup

**Files:**
- Modify: `crates/quotio-core/src/codex_launch.rs:559-704`
- Test: `crates/quotio-core/src/codex_launch.rs` test module

- [ ] **Step 1: Add failing metadata round-trip and legacy-backup tests**

Add tests that use a temporary Codex home:

```rust
#[test]
fn launch_backup_round_trips_prepared_then_marks_running_with_pid() {
    let home = temp_codex_home("ql_codex_launch_session");
    std::fs::create_dir_all(&home).unwrap();
    let state = CodexLaunchSessionState {
        profile_id: "profile-b".to_string(),
        account_key: "codex-b".to_string(),
        launch_mode: "app".to_string(),
        pid: None,
        phase: CodexLaunchPhase::Prepared,
    };

    write_launch_backup_in(&home, Some(state.clone())).unwrap();
    assert_eq!(
        load_launch_backup_status_in(&home).unwrap(),
        LaunchBackupStatus::Managed(state)
    );

    mark_launch_session_running_in(&home, Some(4242)).unwrap();
    let LaunchBackupStatus::Managed(recovered) = load_launch_backup_status_in(&home).unwrap() else {
        panic!("managed launch state expected");
    };
    assert_eq!(recovered.pid, Some(4242));
    assert_eq!(recovered.phase, CodexLaunchPhase::Running);
    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn legacy_launch_backup_without_session_metadata_still_restores() {
    let home = temp_codex_home("ql_codex_legacy_launch_backup");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::write(home.join("auth.json"), "quotio-auth").unwrap();
    std::fs::write(home.join("config.toml"), "quotio-config").unwrap();
    std::fs::write(
        home.join(LAUNCH_BACKUP_FILE),
        r#"{"auth_json":"original-auth","config_toml":"original-config"}"#,
    )
    .unwrap();

    assert_eq!(load_launch_backup_status_in(&home).unwrap(), LaunchBackupStatus::Legacy);
    let before = std::fs::read_to_string(home.join(LAUNCH_BACKUP_FILE)).unwrap();
    assert!(mark_launch_session_running_in(&home, Some(99)).is_err());
    assert_eq!(std::fs::read_to_string(home.join(LAUNCH_BACKUP_FILE)).unwrap(), before);
    restore_codex_state_from_launch_backup_in(&home).unwrap();
    assert_eq!(std::fs::read_to_string(home.join("auth.json")).unwrap(), "original-auth");
    assert_eq!(std::fs::read_to_string(home.join("config.toml")).unwrap(), "original-config");
    let _ = std::fs::remove_dir_all(&home);
}
```

- [ ] **Step 2: Run the new tests and verify they fail**

Run:

```powershell
cargo test -p quotio-core launch_backup_round_trips_prepared_then_marks_running_with_pid -- --nocapture
cargo test -p quotio-core managed_backup_without_phase_defaults_to_prepared -- --nocapture
cargo test -p quotio-core legacy_launch_backup_without_session_metadata_still_restores -- --nocapture
```

Expected: compilation fails because `CodexLaunchPhase`, the new backup parameter, and transactional metadata helpers do not exist.

- [ ] **Step 3: Add the serializable session state and backup field**

Implement:

```rust
#[derive(Debug, Clone, Copy, Default, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CodexLaunchPhase {
    #[default]
    Prepared,
    Running,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub(crate) struct CodexLaunchSessionState {
    pub profile_id: String,
    pub account_key: String,
    pub launch_mode: String,
    pub pid: Option<u32>,
    #[serde(default)]
    pub phase: CodexLaunchPhase,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LaunchBackupStatus {
    Missing,
    Legacy,
    Managed(CodexLaunchSessionState),
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct CodexLaunchBackup {
    auth_json: Option<String>,
    config_toml: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    session: Option<CodexLaunchSessionState>,
}
```

Change `write_launch_backup_in` to accept `Option<CodexLaunchSessionState>`, keep `write_launch_backup()` as the metadata-free compatibility entry point, and atomically write the token-bearing backup:

```rust
fn write_launch_backup_in(
    home: &Path,
    session: Option<CodexLaunchSessionState>,
) -> Result<(), String> {
    std::fs::create_dir_all(home).map_err(|e| format!("创建 ~/.codex 失败: {e}"))?;
    let backup_path = home.join(LAUNCH_BACKUP_FILE);
    if backup_path.exists() {
        return Err(format!("Codex 启动备份已存在: {}", backup_path.display()));
    }
    let (auth_json, config_toml) = read_codex_state_in(home);
    let backup = CodexLaunchBackup { auth_json, config_toml, session };
    let text = serde_json::to_string_pretty(&backup)
        .map_err(|e| format!("序列化 Codex 启动备份失败: {e}"))?;
    quotio_platform::atomic_write(&backup_path, text.as_bytes(), true)
        .map_err(|e| format!("写入 Codex 启动备份失败: {e}"))
}

pub(crate) fn write_launch_backup_for_session_unlocked(
    session: CodexLaunchSessionState,
) -> Result<(), String> {
    write_launch_backup_in(&codex_home(), Some(session))
}

pub(crate) fn load_launch_backup_status_unlocked() -> Result<LaunchBackupStatus, String> {
    load_launch_backup_status_in(&codex_home())
}

fn load_launch_backup_status_in(home: &Path) -> Result<LaunchBackupStatus, String> {
    let path = home.join(LAUNCH_BACKUP_FILE);
    if !path.exists() {
        return Ok(LaunchBackupStatus::Missing);
    }
    let text = std::fs::read_to_string(&path)
        .map_err(|e| format!("读取 Codex 启动备份失败 {}: {e}", path.display()))?;
    let backup: CodexLaunchBackup = serde_json::from_str(&text)
        .map_err(|e| format!("解析 Codex 启动备份失败: {e}"))?;
    Ok(match backup.session {
        Some(session) => LaunchBackupStatus::Managed(session),
        None => LaunchBackupStatus::Legacy,
    })
}

pub(crate) fn mark_launch_session_running_unlocked(pid: Option<u32>) -> Result<(), String> {
    mark_launch_session_running_in(&codex_home(), pid)
}

fn mark_launch_session_running_in(home: &Path, pid: Option<u32>) -> Result<(), String> {
    let path = home.join(LAUNCH_BACKUP_FILE);
    let text = std::fs::read_to_string(&path)
        .map_err(|e| format!("读取 Codex 启动备份失败 {}: {e}", path.display()))?;
    let mut backup: CodexLaunchBackup = serde_json::from_str(&text)
        .map_err(|e| format!("解析 Codex 启动备份失败: {e}"))?;
    let session = backup.session.as_mut()
        .ok_or_else(|| "Codex 启动备份缺少会话元数据，无法标记运行状态".to_string())?;
    session.phase = CodexLaunchPhase::Running;
    session.pid = pid;
    let updated = serde_json::to_string_pretty(&backup)
        .map_err(|e| format!("序列化 Codex 启动备份失败: {e}"))?;
    quotio_platform::atomic_write(&path, updated.as_bytes(), true)
        .map_err(|e| format!("更新 Codex 启动会话失败 {}: {e}", path.display()))
}
```

- [ ] **Step 4: Update existing backup tests and run the focused suite**

Change existing test calls from `write_launch_backup_in(&home)` to `write_launch_backup_in(&home, None)`.
Also add `managed_backup_without_phase_defaults_to_prepared`: deserialize managed
metadata that has a pid but no phase and assert `phase == CodexLaunchPhase::Prepared`.

Run:

```powershell
cargo test -p quotio-core codex_launch::tests -- --nocapture
```

Expected: every `codex_launch::tests` test passes.

- [ ] **Step 5: Commit the metadata layer**

Run:

```powershell
git add crates/quotio-core/src/codex_launch.rs
git commit -m "fix(codex): persist managed launch session metadata"
```

### Task 2: Reconcile stale login-only markers safely

**Files:**
- Modify: `crates/quotio-core/src/codex_launch.rs:430-535`
- Modify: `crates/quotio-platform/src/lib.rs:402-550`
- Modify: `crates/quotio-platform/Cargo.toml`
- Modify: `Cargo.lock`
- Test: `crates/quotio-core/src/codex_launch.rs` test module
- Test: `crates/quotio-platform/src/lib.rs` test module

- [ ] **Step 1: Add a failing retain-one then clear-all reconciliation test**

Add:

```rust
#[test]
fn reconciling_bound_accounts_retains_active_then_clears_all() {
    let dir = temp_codex_home("ql_codex_bound_release_all");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("codex-a.json"),
        r#"{"type":"codex","disabled":true,"quotio_bound_login_only":true,"quotio_previous_disabled":false}"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("codex-b.json"),
        r#"{"type":"codex","disabled":true,"quotio_bound_login_only":true,"quotio_previous_disabled":true}"#,
    )
    .unwrap();
    std::fs::write(dir.join("unrelated.json"), "not-json").unwrap();

    assert_eq!(reconcile_login_only_markers_in(&dir, Some("codex-b")).unwrap(), 1);
    let a = read_json_for_test(&dir.join("codex-a.json"));
    let b = read_json_for_test(&dir.join("codex-b.json"));
    assert_eq!(a["disabled"], false);
    assert_eq!(b["disabled"], true);
    assert!(a.get("quotio_bound_login_only").is_none());
    assert_eq!(b["quotio_bound_login_only"], true);

    assert_eq!(reconcile_login_only_markers_in(&dir, None).unwrap(), 1);
    let b = read_json_for_test(&dir.join("codex-b.json"));
    assert_eq!(b["disabled"], true);
    assert!(b.get("quotio_bound_login_only").is_none());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn managed_disk_cleanup_restores_backup_and_clears_markers() {
    let home = temp_codex_home("ql_codex_managed_cleanup_home");
    let auth_dir = temp_codex_home("ql_codex_managed_cleanup_auth");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&auth_dir).unwrap();
    std::fs::write(home.join("auth.json"), "original-auth").unwrap();
    write_launch_backup_in(&home, None).unwrap();
    std::fs::write(home.join("auth.json"), "quotio-auth").unwrap();
    std::fs::write(
        auth_dir.join("codex-a.json"),
        r#"{"disabled":true,"quotio_bound_login_only":true,"quotio_previous_disabled":false}"#,
    )
    .unwrap();

    assert_eq!(cleanup_managed_codex_disk_state_in(&home, &auth_dir).unwrap(), 1);
    assert_eq!(std::fs::read_to_string(home.join("auth.json")).unwrap(), "original-auth");
    assert!(!home.join(LAUNCH_BACKUP_FILE).exists());
    let account = read_json_for_test(&auth_dir.join("codex-a.json"));
    assert_eq!(account["disabled"], false);
    assert!(account.get("quotio_bound_login_only").is_none());
    let _ = std::fs::remove_dir_all(&home);
    let _ = std::fs::remove_dir_all(&auth_dir);
}
```

- [ ] **Step 2: Run the test and verify it fails**

Run:

```powershell
cargo test -p quotio-core reconciling_bound_accounts_retains_active_then_clears_all -- --nocapture
cargo test -p quotio-core managed_disk_cleanup_restores_backup_and_clears_markers -- --nocapture
```

Expected: compilation fails because the sweep helper does not exist.

- [ ] **Step 3: Implement the sweep helper**

Add a locked entry point and a directory-scoped implementation. The implementation must read `.json` extensions case-insensitively, skip text without `quotio_bound_login_only`, report malformed matching JSON instead of silently ignoring it, continue after individual failures, retain the selected key case-insensitively, and return one combined error after attempting every matching file:

```rust
pub(crate) fn reconcile_login_only_markers_unlocked(
    retain_key: Option<&str>,
) -> Result<usize, String> {
    reconcile_login_only_markers_in(&proxy_auth_dir(), retain_key)
}

fn reconcile_login_only_markers_in(
    dir: &Path,
    retain_key: Option<&str>,
) -> Result<usize, String> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(error) => return Err(format!(
            "读取 CLIProxyAPI 账号目录失败 {}: {error}",
            dir.display()
        )),
    };
    let retain_key = retain_key.map(str::trim).filter(|key| !key.is_empty());
    let mut released = 0usize;
    let mut errors = Vec::new();
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                errors.push(format!("读取代理账号条目失败: {error}"));
                continue;
            }
        };
        let path = entry.path();
        let is_json = path.extension().and_then(OsStr::to_str)
            .is_some_and(|extension| extension.eq_ignore_ascii_case("json"));
        if !is_json {
            continue;
        }
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) => {
                errors.push(format!("读取账号文件失败 {}: {error}", path.display()));
                continue;
            }
        };
        if !text.contains(BOUND_LOGIN_ONLY_FIELD) {
            continue;
        }
        let value = match serde_json::from_str::<Value>(&text) {
            Ok(value) => value,
            Err(error) => {
                errors.push(format!("解析账号文件失败 {}: {error}", path.display()));
                continue;
            }
        };
        let Some(key) = path.file_stem().and_then(OsStr::to_str) else {
            errors.push(format!("账号文件名无法识别: {}", path.display()));
            continue;
        };
        if retain_key.is_some_and(|retain| retain.eq_ignore_ascii_case(key)) {
            continue;
        }
        let is_marked = value.get(BOUND_LOGIN_ONLY_FIELD)
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if !is_marked {
            continue;
        }
        match release_bound_account_login_only_at(&path) {
            Ok(true) => released += 1,
            Ok(false) => {},
            Err(error) => errors.push(error),
        }
    }
    if errors.is_empty() {
        Ok(released)
    } else {
        Err(format!("清理 Codex 登录专用账号标记失败:\n{}", errors.join("\n")))
    }
}

pub(crate) fn cleanup_managed_codex_disk_state_unlocked() -> Result<usize, String> {
    cleanup_managed_codex_disk_state_in(&codex_home(), &proxy_auth_dir())
}

fn cleanup_managed_codex_disk_state_in(
    home: &Path,
    auth_dir: &Path,
) -> Result<usize, String> {
    let reconciliation = reconcile_login_only_markers_in(auth_dir, None);
    let backup_path = home.join(LAUNCH_BACKUP_FILE);
    let restore = match backup_path.try_exists() {
        Ok(true) => restore_codex_state_from_launch_backup_in(home),
        Ok(false) => Ok(()),
        Err(error) => Err(format!(
            "检查 Codex 启动备份失败 {}: {error}",
            backup_path.display()
        )),
    };
    match (reconciliation, restore) {
        (Ok(count), Ok(())) => Ok(count),
        (Err(marker_error), Ok(())) => Err(marker_error),
        (Ok(_), Err(restore_error)) => Err(restore_error),
        (Err(marker_error), Err(restore_error)) => Err(format!(
            "{marker_error}\n恢复 Codex 启动备份失败: {restore_error}"
        )),
    }
}
```

Harden `quotio_platform::atomic_write` at the same time: create sensitive Unix
temp files with mode `0600`, keep the temp path armed in an RAII guard until a
successful replacement, and use this Windows replacement primitive:

```rust
MoveFileExW(
    source.as_ptr(),
    target.as_ptr(),
    MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
)
```

The guard's `Drop` closes and removes an armed temp path, so failed replacement
does not leak a credential-bearing `.quotio-tmp-*` file.

- [ ] **Step 4: Run marker tests**

Run:

```powershell
cargo test -p quotio-core bound_account -- --nocapture
cargo test -p quotio-core reconciling_bound_accounts_retains_active_then_clears_all -- --nocapture
cargo test -p quotio-core managed_disk_cleanup_restores_backup_and_clears_markers -- --nocapture
cargo test -p quotio-core managed_disk_cleanup_restores_backup_after_marker_error -- --nocapture
cargo test -p quotio-core managed_disk_cleanup_aggregates_marker_and_restore_errors -- --nocapture
cargo test -p quotio-platform atomic_write -- --nocapture
cargo test -p quotio-platform replace_file -- --nocapture
```

Expected: marker restoration, combined-error coverage, durable replacement, temp-file cleanup, and sensitive-file permissions pass.

- [ ] **Step 5: Commit stale-marker cleanup**

Run:

```powershell
git add crates/quotio-core/src/codex_launch.rs crates/quotio-platform/src/lib.rs crates/quotio-platform/Cargo.toml Cargo.lock
git commit -m "fix(codex): clean stale login-only account markers"
```

### Task 3: Recover AppCore state and use one cleanup path

**Files:**
- Modify: `crates/quotio-core/src/lib.rs:132-235`
- Modify: `crates/quotio-core/src/lib.rs:341-368`
- Modify: `crates/quotio-core/src/lib.rs:1038-1233`
- Modify: `crates/quotio-core/src/lib.rs:1262-1336`
- Test: `crates/quotio-core/src/lib.rs` test module

- [ ] **Step 1: Add a failing pure recovery test**

Extract startup conversion into a pure helper and test it:

```rust
#[test]
fn recovered_launch_state_restores_profile_account_mode_but_discards_pid() {
    let state = codex_launch::CodexLaunchSessionState {
        profile_id: "profile-b".to_string(),
        account_key: "codex-b".to_string(),
        launch_mode: "cli".to_string(),
        pid: Some(7331),
        phase: codex_launch::CodexLaunchPhase::Running,
    };
    let (session, profile_id, account_key) = recovered_codex_runtime(Some(state));
    let session = session.unwrap();
    assert_eq!(session.launch_mode, "cli");
    assert_eq!(session.pid, None);
    assert!(session.recovered);
    assert_eq!(profile_id.as_deref(), Some("profile-b"));
    assert_eq!(account_key.as_deref(), Some("codex-b"));
}
```

- [ ] **Step 2: Run the test and verify it fails**

Run:

```powershell
cargo test -p quotio-core recovered_launch_state_restores_profile_account_mode_but_discards_pid -- --nocapture
```

Expected: compilation fails because `recovered_codex_runtime` and `codex_active_account_key` do not exist.

- [ ] **Step 3: Add durable runtime recovery to AppCore**

Add the field:

```rust
codex_active_account_key: Option<String>,
codex_cleanup_pending: bool,
```

Add the pure converter:

```rust
fn recovered_codex_runtime(
    state: Option<codex_launch::CodexLaunchSessionState>,
) -> (Option<codex_launch::CodexSession>, Option<String>, Option<String>) {
    match state {
        Some(state) if state.phase == codex_launch::CodexLaunchPhase::Running => (
            Some(codex_launch::CodexSession::recovered(&state.launch_mode)),
            Some(state.profile_id),
            Some(state.account_key),
        ),
        None => (None, None, None),
        Some(_) => (None, None, None),
    }
}
```

Add a production startup loader returning `(session, profile_id, account_key,
cleanup_pending)` with these exact branches:

- `Missing`: reconcile all markers; failure sets pending.
- `Legacy`: recover no runtime and preserve the backup for Stop/Start cleanup.
- `Managed(Prepared)`: call `close_codex_app()`, wait 400 ms, then call
  `cleanup_managed_codex_disk_state_unlocked()` regardless of
  `keep_proxy_on_exit`; success is fully inactive, while failure keeps the
  persisted profile/account and sets pending.
- `Managed(Running)`: retain the active account marker, recover profile/account/
  mode with runtime `pid=None`, and set pending if reconciliation fails.
- read/parse error: preserve disk state and set pending.

The `cfg(test)` loader returns an empty four-tuple so `AppCore::default()` never
touches the developer's real auth directories. Initialize
`codex_session_generation` to `1` only when a runtime session was recovered.

- [ ] **Step 4: Centralize lifecycle cleanup**

Replace profile-based release with one finish helper. It terminates only a
runtime pid when available, closes the app when a session or backup exists,
always attempts marker cleanup and backup restoration through
`cleanup_managed_codex_disk_state_unlocked`, bumps the generation, and reconciles
the scheduler. Cleanup success clears identity/pending; cleanup failure clears
the runtime session but retains profile/account identity and sets pending:

```rust
fn finish_codex_session_unlocked(
    &mut self,
    terminate_process: bool,
) -> Result<usize, ManagementCoreError> {
    let session = self.codex_session.take();
    if terminate_process {
        if let Some(pid) = session.as_ref().and_then(|session| session.pid) {
            codex_launch::kill_process(pid);
        }
        if session.is_some() || codex_launch::launch_backup_exists() {
            codex_launch::close_codex_app();
            thread::sleep(Duration::from_millis(400));
        }
    }
    let cleanup = codex_launch::cleanup_managed_codex_disk_state_unlocked()
        .map_err(ManagementCoreError::Unavailable);
    apply_codex_cleanup_outcome(
        &cleanup,
        &mut self.codex_active_profile_id,
        &mut self.codex_active_account_key,
        &mut self.codex_cleanup_pending,
    );
    self.codex_session_generation = self.codex_session_generation.wrapping_add(1);
    let _ = self.scheduler_reconcile();
    cleanup
}
```

Use this cleanup in explicit Stop, launch replacement, non-keep shutdown, and monitor-confirmed exit. Launch-failure rollback calls the same disk cleanup after explicitly killing any just-started process. `keep_proxy_on_exit=true` returns before this helper and therefore preserves the recovered session.

Define active state as `codex_session.is_some() || codex_cleanup_pending ||
launch_backup_exists()`. Monitor cleanup returns `true` only when disk cleanup
succeeds; on failure it logs, preserves identity, sets pending, and does not
falsely report an automatic restore. Recovered App sessions clean up after two
consecutive misses, while fresh sessions never observed alive remain
conservative; recovered CLI has no pid probe.

- [ ] **Step 5: Write session metadata during launch**

Compute `mode` before writing the backup and replace the no-metadata call with:

```rust
let persisted = codex_launch::CodexLaunchSessionState {
    profile_id: profile_id.to_string(),
    account_key: account_key.clone(),
    launch_mode: mode.to_string(),
    pid: None,
    phase: codex_launch::CodexLaunchPhase::Prepared,
};
codex_launch::write_launch_backup_for_session_unlocked(persisted)
    .map_err(ManagementCoreError::Unavailable)?;
```

After launch returns `pid`, persist it before exposing success:

```rust
if let Err(error) = codex_launch::mark_launch_session_running_unlocked(pid) {
    if let Some(pid) = pid {
        codex_launch::kill_process(pid);
    }
    codex_launch::close_codex_app();
    return Err(ManagementCoreError::Unavailable(error));
}
self.codex_session = Some(codex_launch::CodexSession::new(pid, &mode));
self.codex_session_generation = self.codex_session_generation.wrapping_add(1);
self.codex_active_profile_id = Some(profile_id.to_string());
self.codex_active_account_key = Some(account_key.clone());
self.codex_cleanup_pending = false;
```

If the Running+pid commit fails, kill/close the just-launched process, wait 400
ms, and enter the existing launch rollback. That rollback calls
`cleanup_managed_codex_disk_state_unlocked()`, preserves identity plus pending on
cleanup failure, bumps `codex_session_generation`, reconciles the scheduler, and
appends any cleanup failure to the original launch error.

At the beginning of `codex_start`, use `self.codex_active()` rather than only
`self.codex_session.is_some()`. A recovered App same-profile session may return
idempotently, but a recovered CLI session and any cleanup-pending state must not;
they call `codex_stop_unlocked()` before relaunching.

After confirming there is no active managed session, unconditionally run:

```rust
codex_launch::reconcile_login_only_markers_unlocked(None)
    .map_err(ManagementCoreError::Unavailable)?;
```

Only then mark the selected account. This catches orphan markers even when an older partial cleanup already removed the launch backup.

- [ ] **Step 6: Make explicit Stop clean legacy markers even without metadata**

In `codex_stop_unlocked`, do not return before calling `finish_codex_session_unlocked(true)`. Record whether a session or backup existed before cleanup. If neither existed but cleanup releases accounts, return `"已清理残留的 Codex 登录账号状态"`; if it releases none, retain `"Codex 未在运行"`; otherwise return `"已停止 Codex 并还原配置"`.

- [ ] **Step 7: Run lifecycle tests**

Run:

```powershell
cargo test -p quotio-core prepared_cleanup -- --nocapture
cargo test -p quotio-core recovered_launch_state_restores_profile_account_mode_but_discards_pid -- --nocapture
cargo test -p quotio-core recovered_monitor -- --nocapture
cargo test -p quotio-core fresh_unseen_monitor_never_finishes_on_misses -- --nocapture
cargo test -p quotio-core cleanup_failure_preserves_identity_marks_pending_and_is_not_reported_complete -- --nocapture
cargo test -p quotio-core recovered_cli_and_cleanup_pending_cannot_satisfy_same_profile_start -- --nocapture
cargo test -p quotio-core codex_launch::tests -- --nocapture
```

Expected: recovery, monitor debounce, backup compatibility, and marker cleanup tests all pass.

- [ ] **Step 8: Commit lifecycle integration**

Run:

```powershell
git add crates/quotio-core/src/lib.rs crates/quotio-core/src/codex_launch.rs
git commit -m "fix(codex): recover login-only sessions across restart"
```

### Task 4: Verify implementation and obtain code review

**Files:**
- Verify: `crates/quotio-core/src/codex_launch.rs`
- Verify: `crates/quotio-core/src/lib.rs`
- Verify: `crates/quotio-platform/src/lib.rs`
- Verify: `crates/quotio-platform/Cargo.toml`

- [ ] **Step 1: Format and run focused tests**

Run:

```powershell
cargo fmt --all
cargo test -p quotio-core codex_launch::tests -- --nocapture
cargo test -p quotio-core prepared_ -- --nocapture
cargo test -p quotio-core recovered_ -- --nocapture
cargo test -p quotio-core codex_monitor -- --nocapture
cargo test -p quotio-platform replace_file -- --nocapture
cargo test -p quotio-platform atomic_write -- --nocapture
```

Expected: formatting succeeds and every focused test passes.

- [ ] **Step 2: Run the complete Rust and frontend verification**

Run:

```powershell
cargo test -p quotio-core -p quotio-platform
cargo check --workspace
npm --prefix apps/desktop test
npm run web:build
git diff --check
```

Expected: Rust tests and checks pass, Vitest passes, the TypeScript/Vite production build succeeds, and `git diff --check` prints nothing.

- [ ] **Step 3: Request independent specification and quality reviews**

Dispatch one reviewer to compare the implementation against the design and plan, then a second reviewer to inspect error handling, migration safety, file-write atomicity, and unintended release of user-disabled accounts.

Expected: both reviewers return `APPROVED`, or every finding is fixed and re-reviewed.

- [ ] **Step 4: Verify branch contents**

Run:

```powershell
git status --short
git log --oneline origin/main..HEAD
git diff --stat origin/main...HEAD
```

Expected: the tree is clean and contains only PowerShell compatibility, login-only lifecycle recovery, their docs/tests, and release metadata once added.

### Task 5: Prepare v0.7.1 metadata and local release build

**Files:**
- Modify: `package.json`
- Modify: `apps/desktop/package.json`
- Modify: `apps/desktop/src-tauri/Cargo.toml`
- Modify: `apps/desktop/src-tauri/tauri.conf.json`
- Modify: `Cargo.lock`
- Modify: `README.md`
- Modify: `CHANGELOG.md`

- [ ] **Step 1: Set all application versions to 0.7.1**

Run:

```powershell
npm run version:set -- 0.7.1
```

Expected: the four application version files and README download links change from `0.7.0` to `0.7.1`.

Run `cargo check -p quotio-desktop` once after the version script so `Cargo.lock` records `quotio-desktop` version `0.7.1`.

- [ ] **Step 2: Add the v0.7.1 changelog at the top**

Insert this section before v0.7.0:

```markdown
## v0.7.1 - 2026-07-17

修复 Windows 商店版 Codex 启动兼容性，并解决多个启动方案在 Quotio 重启后残留“Codex 登录专用”状态的问题。

### 修复

- **修复部分 Windows 环境启动 Codex 时提示“调用 PowerShell 失败: program not found”**：不再依赖 Quotio 进程继承到的 PATH，优先使用系统 Windows PowerShell，并兼容 PowerShell 7 与 `pwsh` 兜底；商店入口探测、AppsFolder 激活、进程清理、PID 探测和 CLI 启动统一使用同一套解析逻辑。
- **修复开启“退出时保留代理”后，重启 Quotio 再切换第二个 Codex 启动方案会留下多个“Codex 登录专用”账号**：启动备份现在持久化当前方案、账号、启动模式和 Prepared/Running 提交状态；重启后只恢复已提交会话的身份且不会复用旧 PID，停止、切换、启动失败和正常清理时会恢复账号绑定前的启用状态，并清理旧版本遗留标记。
```

- [ ] **Step 3: Verify version and changelog consistency**

Run:

```powershell
rg -n '0\.7\.1' package.json apps/desktop/package.json apps/desktop/src-tauri/Cargo.toml apps/desktop/src-tauri/tauri.conf.json README.md CHANGELOG.md
rg -n 'name = "quotio-desktop"|version = "0\.7\.1"' Cargo.lock
rg -n '0\.7\.0' package.json apps/desktop/package.json apps/desktop/src-tauri/Cargo.toml apps/desktop/src-tauri/tauri.conf.json
```

Expected: all release version files contain `0.7.1`; the second command returns no matches.

- [ ] **Step 4: Build the Windows release and portable package locally**

Run:

```powershell
npm run release
```

Expected: Tauri produces the Windows installer artifacts and `package-portable.ps1` produces the v0.7.1 portable archive without rebuilding an older binary.

- [ ] **Step 5: Commit release metadata**

Run:

```powershell
git add package.json apps/desktop/package.json apps/desktop/src-tauri/Cargo.toml apps/desktop/src-tauri/tauri.conf.json Cargo.lock README.md CHANGELOG.md
git commit -m "release: v0.7.1"
```

### Task 6: Push, publish, and verify v0.7.1

**Files:**
- Verify: `.github/workflows/release.yml`
- Verify remote: GitHub Release `v0.7.1`

- [ ] **Step 1: Reconcile with origin/main before publishing**

Run:

```powershell
git fetch origin --tags
git log --oneline --left-right origin/main...HEAD
```

Expected: no unexpected remote commits are missing. If origin/main advanced, rebase the release branch and rerun Task 4 verification.

- [ ] **Step 2: Create the tag and atomically publish main plus the tag**

Run:

```powershell
git tag -a v0.7.1 -m "Quotio v0.7.1"
if ((git rev-list -n 1 v0.7.1) -ne (git rev-parse HEAD)) {
  throw "v0.7.1 没有指向当前发布提交"
}
git push --atomic origin HEAD:main v0.7.1
```

Expected: `origin/main` and tag `v0.7.1` appear together, and GitHub Actions starts the `Release` workflow for the tag.

- [ ] **Step 3: Monitor the workflow to completion**

Run:

```powershell
$runId = $null
for ($i = 0; $i -lt 24 -and -not $runId; $i++) {
  $runId = gh run list --repo xiaocoss/quotio-desktop `
    --workflow release.yml --limit 10 --json databaseId,headBranch |
    ConvertFrom-Json |
    Where-Object headBranch -eq 'v0.7.1' |
    Select-Object -First 1 -ExpandProperty databaseId
  if (-not $runId) { Start-Sleep -Seconds 5 }
}
if (-not $runId) { throw '未找到 v0.7.1 Release 工作流' }
gh run watch $runId --repo xiaocoss/quotio-desktop --exit-status
```

Expected: Windows, macOS, Linux, and `rewrite-manifest` jobs all succeed.

- [ ] **Step 4: Verify release notes and assets**

Run:

```powershell
$release = gh release view v0.7.1 --repo xiaocoss/quotio-desktop `
  --json url,isDraft,isPrerelease,body,assets | ConvertFrom-Json
$expected = @(
  'latest.json',
  'Quotio-0.7.1-1.x86_64.rpm', 'Quotio-0.7.1-1.x86_64.rpm.sig',
  'Quotio_0.7.1_amd64.AppImage', 'Quotio_0.7.1_amd64.AppImage.sig',
  'Quotio_0.7.1_amd64.deb', 'Quotio_0.7.1_amd64.deb.sig',
  'Quotio_0.7.1_universal.dmg',
  'Quotio_0.7.1_x64-setup.exe', 'Quotio_0.7.1_x64-setup.exe.sig',
  'Quotio_0.7.1_x64_en-US.msi', 'Quotio_0.7.1_x64_en-US.msi.sig',
  'Quotio_universal.app.tar.gz', 'Quotio_universal.app.tar.gz.sig'
)
if (Compare-Object $expected @($release.assets.name)) { throw 'Release 资产不完整' }
if ($release.isDraft -or $release.isPrerelease) { throw 'Release 状态错误' }
if (-not $release.body.StartsWith('## v0.7.1 - 2026-07-17')) {
  throw 'Release 更新日志抽取失败'
}
```

Expected: the release is public, has the exact 14 signed/update assets, and its body contains the v0.7.1 changelog.

- [ ] **Step 5: Verify updater manifest URLs**

Run:

```powershell
$manifest = Invoke-RestMethod `
  'https://github.com/xiaocoss/quotio-desktop/releases/download/v0.7.1/latest.json'
if ($manifest.version -ne '0.7.1') { throw 'latest.json 版本错误' }
if (-not $manifest.notes.StartsWith('## v0.7.1 - 2026-07-17')) {
  throw 'latest.json 更新日志错误'
}
foreach ($entry in $manifest.platforms.PSObject.Properties) {
  if (-not $entry.Value.signature) { throw "$($entry.Name) 缺少签名" }
  if ($entry.Value.url -notmatch '/releases/download/v0\.7\.1/') {
    throw "$($entry.Name) 下载地址版本错误"
  }
}
```

Expected: every platform URL points to the configured ghproxy prefix followed by the GitHub v0.7.1 asset URL.
