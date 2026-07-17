# Codex Login-Only Session Recovery Design

## Problem

Quotio marks the account used to sign in to a managed Codex launch with
`quotio_bound_login_only=true` and `disabled=true`. The marker intentionally
keeps that account out of the proxy rotation pool while Codex is using it for
login.

When `keep_proxy_on_exit=true`, closing Quotio preserves the proxy, Codex launch
backup, and login-only marker. However, the next Quotio process initializes
`codex_session` and `codex_active_profile_id` to `None`. The disk state survives
but the in-memory identity of the active profile and account does not.

This produces several incorrect outcomes:

- stopping Codex after restarting Quotio cannot identify the account to release;
- starting a second launch profile restores the old Codex backup but does not
  release the first profile's marker before marking the second account;
- multiple accounts can remain disabled and appear as "Codex 登录专用";
- editing or deleting the original profile can permanently remove the only
  in-memory lookup path that the release code relies on.

Creating two profiles is therefore a trigger for the visible double-marker
symptom, but the root cause is that launch-session identity is not durable.

## Goals

- Persist enough launch-session identity to recover a managed Codex session
  after Quotio restarts.
- Keep exactly one login-only account while one managed Codex session is active.
- Release the account by its durable account key instead of looking it up through
  mutable profile settings.
- Clean markers left by v0.7.0 and earlier when the old managed session is ended.
- Preserve the existing meaning of `keep_proxy_on_exit`.
- Keep old `quotio-launch-backup.json` files readable.

## Non-goals

- Supporting multiple simultaneous Quotio-managed Codex sessions.
- Detecting or managing Codex instances that were launched outside Quotio.
- Changing quota-card visuals or the meaning of the login-only badge.
- Re-enabling a user-disabled account; cleanup must restore its previous
  `disabled` value.

## Considered Approaches

### 1. Release every marker whenever Quotio starts

This is small, but it violates `keep_proxy_on_exit`: a still-running Codex
session would lose its protected login account as soon as Quotio reopened.

### 2. Persist launch-session metadata and reconcile stale markers

Store the profile id, account key, launch mode, and optional pid beside the
existing Codex launch backup. Rehydrate the in-memory session on startup and use
the durable account key for release. Sweep all Quotio-owned login-only markers
only when the managed session is actually being ended.

This is the selected approach. It preserves the keep-running behavior, fixes
cross-restart stopping and profile switching, and provides a safe migration path
for old residual markers.

### 3. Infer the active account from `~/.codex/auth.json`

This avoids new metadata, but it depends on token and schema comparisons after
Codex may have refreshed or rewritten authentication. It is less reliable than
persisting the identity Quotio already knows at launch time.

## Data Model

Extend `CodexLaunchBackup` with an optional session object:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CodexLaunchSessionState {
    pub profile_id: String,
    pub account_key: String,
    pub launch_mode: String,
    pub pid: Option<u32>,
}

#[derive(Debug, Serialize, Deserialize)]
struct CodexLaunchBackup {
    auth_json: Option<String>,
    config_toml: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    session: Option<CodexLaunchSessionState>,
}
```

The optional field makes backups created by earlier releases remain valid.
Session metadata contains no token or API key.

`AppCore` gains `codex_active_account_key: Option<String>`. This value is set
when a launch starts and recovered from the backup after restart. Profile id is
kept for UI state, while account key is the authoritative release identity.

## Launch Flow

1. If an older managed session or launch backup exists, end it first: close the
   old Codex process, release every Quotio login-only marker, restore the Codex
   files, and reconcile the scheduler.
2. Mark the selected account login-only unless `absorb_bound_account` is enabled.
3. Write the original Codex files and session metadata to the launch backup.
4. Configure and launch Codex.
5. Persist the returned pid into the session metadata before reporting success.
6. Set the in-memory session, active profile id, and active account key.

Any failure after marking the account releases all Quotio login-only markers,
restores an existing backup, clears in-memory session identity, and reconciles
the scheduler.

## Startup Recovery

When `AppCore` is created:

- if a launch backup contains session metadata, rebuild `CodexSession` from its
  mode and pid, restore `codex_active_profile_id`, and restore
  `codex_active_account_key`;
- if no launch backup exists, any login-only marker is unambiguously stale and
  is released during startup;
- if a legacy backup exists without session metadata, do not guess which active
  account to retain. Keep the backup untouched until the user stops or starts a
  managed session; that ending path safely releases all old markers.

For a recovered app session, monitoring retains the existing conservative
`seen_running` behavior so Store-app process-name mismatches do not cause an
automatic false shutdown. Explicit Stop remains authoritative.

## Ending and Cleanup

All paths that truly end a managed Codex session use one cleanup operation:

- explicit Stop;
- switching to another profile;
- normal Quotio shutdown when `keep_proxy_on_exit=false`;
- monitor-confirmed Codex exit;
- launch failure rollback;
- replacement of a legacy backup before a new launch.

The cleanup operation scans proxy auth JSON files for
`quotio_bound_login_only=true`, restores each file's
`quotio_previous_disabled` value, and removes both Quotio marker fields. A sweep
is safe because Quotio supports only one managed Codex session; multiple markers
are stale state, not independent live sessions.

Cleanup uses atomic account-file writes. Errors are logged on best-effort
shutdown/monitor paths and returned to the caller on explicit Stop or launch
replacement paths. Codex-state restoration still runs even if one marker cannot
be released, and the combined error identifies both failures when applicable.

`keep_proxy_on_exit=true` remains the sole exception: shutdown preserves the
session metadata, backup, proxy, Codex process, and the one active marker.

## Legacy Compatibility

- Old backups deserialize with `session=None`.
- Old single or multiple markers are released the next time the legacy session
  is explicitly stopped or replaced.
- Markers without a backup are released on startup because no managed Codex
  configuration remains to protect.
- Cleanup always respects `quotio_previous_disabled`, so accounts disabled by
  the user before binding stay disabled.

## Tests

Add regression coverage for:

- session metadata round-trip and pid update;
- deserializing and restoring a legacy backup without metadata;
- recovering profile id, account key, launch mode, and pid after restart;
- releasing two residual markers while restoring each prior disabled value;
- Stop after restart releasing markers without consulting mutable profile data;
- replacing a legacy backup before launching a second profile;
- startup cleanup when markers exist without a launch backup;
- launch failure removing metadata and markers;
- `keep_proxy_on_exit=true` preserving recovered session state;
- existing monitor debounce and PowerShell fallback tests remaining green.

## Release Scope

This lifecycle fix is included in v0.7.1 together with the Windows PowerShell
path-resolution fix. No rose-theme work or unrelated settings changes are part
of the release.
