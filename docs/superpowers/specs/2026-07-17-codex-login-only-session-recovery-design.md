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

Store the profile id, account key, launch mode, optional pid, and a transactional
launch phase beside the existing Codex launch backup. On startup, recover only
committed (`Running`) sessions, discard the persisted pid from runtime state,
and use the durable account key for release. Sweep all Quotio-owned login-only
markers only when the managed session is actually being ended or an incomplete
(`Prepared`) launch is rolled back.

This is the selected approach. It preserves the keep-running behavior, fixes
cross-restart stopping and profile switching, and provides a safe migration path
for old residual markers.

### 3. Infer the active account from `~/.codex/auth.json`

This avoids new metadata, but it depends on token and schema comparisons after
Codex may have refreshed or rewritten authentication. It is less reliable than
persisting the identity Quotio already knows at launch time.

## Data Model

Extend `CodexLaunchBackup` with an optional session object and make the launch
phase explicit:

```rust
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CodexLaunchPhase {
    #[default]
    Prepared,
    Running,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CodexLaunchSessionState {
    pub profile_id: String,
    pub account_key: String,
    pub launch_mode: String,
    pub pid: Option<u32>,
    #[serde(default)]
    pub phase: CodexLaunchPhase,
}

#[derive(Debug, Serialize, Deserialize)]
struct CodexLaunchBackup {
    auth_json: Option<String>,
    config_toml: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    session: Option<CodexLaunchSessionState>,
}
```

The optional session field makes backups created by earlier releases remain
valid. A managed session written by an intermediate implementation may have no
`phase`; serde defaults that metadata to `Prepared`, which is the safe rollback
classification. Session metadata contains no token or API key.

`AppCore` gains `codex_active_account_key: Option<String>` and
`codex_cleanup_pending: bool`. Profile id is kept for UI state, account key is
the authoritative release identity, and the pending bit keeps failed cleanup
visible and retryable. `codex_active()` is true when a runtime session exists,
cleanup is pending, or a launch backup still exists.

`CodexSession::recovered(mode)` always sets `pid=None`. The persisted pid is an
audit value written with the `Running` commit, not a process handle that may be
trusted after an OS/process restart because pid reuse could target an unrelated
process.

## Launch Flow

1. Validate the requested profile before disturbing an existing valid session.
2. If `codex_active()` is true, return idempotently only when the same profile is
   represented by a usable runtime session. Cleanup-pending state and a recovered
   CLI session are never treated as proof that the requested launch is running;
   they enter Stop/cleanup first.
3. After the old state is ended, run a no-retain marker reconciliation so an
   orphan marker cannot survive merely because its backup was already removed.
4. Mark the selected account login-only unless `absorb_bound_account` is enabled.
5. Write the original Codex files plus session metadata with
   `phase=Prepared` and `pid=None` before changing Codex configuration.
6. Configure and launch Codex.
7. After launch succeeds, call
   `mark_launch_session_running_unlocked(pid)`. This atomically commits
   `phase=Running` and the returned pid in the same backup replacement.
8. Only after that commit succeeds, expose the in-memory session, active profile
   id, active account key, and clear `codex_cleanup_pending`.

Any failure after marking the account runs the same unified disk rollback:
release all Quotio login-only markers and restore/remove the launch backup. If
the Running commit itself fails, the just-launched pid is killed when available,
the Codex app is closed, and the code waits 400 ms before rollback. A successful
rollback clears identity; a failed rollback keeps profile/account identity,
sets `codex_cleanup_pending=true`, and returns a combined launch-plus-cleanup
error so Stop or a later Start can retry.

## Startup Recovery

When `AppCore` is created:

- `Missing`: any login-only marker is unambiguously stale and is released. A
  reconciliation failure sets `codex_cleanup_pending=true`, keeping Stop/Start
  as retry entry points even though there is no recovered identity.
- `Legacy`: do not guess which account is active. Keep the backup and markers
  untouched until explicit Stop or a new Start uses the unified cleanup path.
- `Managed(Prepared)`: the launch transaction never committed. Immediately call
  `close_codex_app()`, wait 400 ms, then run the unified disk rollback regardless
  of `keep_proxy_on_exit`. Success produces a fully inactive runtime. Failure
  keeps the persisted profile/account identity, leaves `codex_session=None`, and
  sets `codex_cleanup_pending=true` so cleanup can be retried.
- `Managed(Running)`: retain the persisted account marker, release other stale
  markers, and recover the profile, account, and launch mode. Never trust or
  reuse the persisted pid; runtime recovery always uses `pid=None`. A marker
  reconciliation failure retains the recovered identity and sets cleanup pending.

A parsed backup is classified as `Missing`, `Legacy`, or `Managed(state)` so a
parse error is never mistaken for a missing backup. Read/parse failures preserve
the disk state, set cleanup pending, and avoid destructive reconciliation.

Unit tests that construct `AppCore::default()` must not run the production
startup reconciliation against the developer's real home directory. Startup
classification and runtime reconstruction are therefore factored into testable
path-scoped and pure helpers, while the test-only default constructor skips
global disk mutation.

Recovered and fresh sessions use deliberately different monitor rules:

- a recovered App session starts with `recovered=true`, `pid=None`, and may be
  ended after two consecutive missing observations even if no alive observation
  has yet occurred;
- one alive observation converts it to the normal `seen_running` state and
  resets the miss counter;
- a fresh session that has never been observed alive remains conservative and
  is not ended merely because startup probes miss it;
- a recovered CLI session has no trustworthy pid and therefore no CLI monitor
  probe. Starting the same CLI profile is not blindly idempotent: it first enters
  cleanup/relaunch instead of asserting that an unverified process is running.

Explicit Stop remains authoritative for all recovered sessions.

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

Before marking a new account, the launch flow also performs an unconditional
no-retain reconciliation after confirming there is no current managed session.
This covers a partial older cleanup where the launch backup was removed but one
account-file write failed, and guarantees every successful launch starts from
zero residual markers.

Cleanup uses atomic account-file writes. Marker reconciliation and Codex backup
restoration are both attempted even when the other one fails, and a combined
error reports both failures when applicable. Explicit Stop and launch
replacement return cleanup errors. Shutdown logs them. Monitor cleanup returns
`true` only when cleanup completed; on failure it does not falsely report an
automatic restore, preserves profile/account identity, and sets cleanup pending.

The shared atomic writer is hardened for these token-bearing files. Sensitive
temporary files are created with owner-only `0600` permissions on Unix instead
of being tightened only after content is written. Windows replacement uses
`MoveFileExW(MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH)` so an existing
target can be replaced durably. An RAII temp-file guard closes and removes the
temporary path on every error path, preventing credential-bearing temp leaks.

`keep_proxy_on_exit=true` remains the normal-shutdown exception: a committed
Running session preserves its metadata, backup, proxy, Codex process, and active
marker. It does not exempt an uncommitted Prepared transaction from startup
rollback.

## Legacy Compatibility

- Old backups deserialize with `session=None`.
- Managed metadata without `phase` defaults to `Prepared` and is rolled back;
  it is never assumed to be a committed running session.
- Old single or multiple markers are released the next time the legacy session
  is explicitly stopped or replaced.
- Markers without a backup are released on startup because no managed Codex
  configuration remains to protect.
- Cleanup always respects `quotio_previous_disabled`, so accounts disabled by
  the user before binding stay disabled.

## Tests

Add regression coverage for:

- Prepared metadata round-trip followed by one atomic Running+pid commit;
- metadata without `phase` defaulting to Prepared;
- deserializing and restoring a legacy backup without session metadata;
- Prepared startup cleanup succeeding to a fully inactive runtime and failing to
  a retained identity plus cleanup-pending state;
- Running recovery restoring profile/account/mode while discarding the pid;
- recovered App cleanup after two misses, transition to normal monitoring after
  an alive observation, and conservative fresh-unseen behavior;
- recovered CLI and cleanup-pending state refusing same-profile idempotence;
- cleanup failure preserving identity, marking pending, and not being reported
  as completed by the monitor;
- releasing residual markers while restoring each prior disabled value;
- disk cleanup continuing backup restoration after marker failure and combining
  marker-plus-restore errors;
- Windows replacement behavior, atomic-write temp cleanup on failure, and Unix
  sensitive-file permissions;
- existing PowerShell fallback and full workspace checks remaining green.

## Release Scope

This lifecycle fix is planned for the v0.7.1 release scope together with the
Windows PowerShell path-resolution fix. The release plan must still complete its
unchecked build, publish, and asset-verification steps before claiming the
release is shipped. No rose-theme work or unrelated settings changes belong in
that scope.
