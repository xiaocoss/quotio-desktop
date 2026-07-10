# Codex Catalog Discovery Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make Quotio generate and reuse `quotio-model-catalog.json` on fresh WindowsApps installations and other Codex layouts without blocking the UI or silently accepting a single broken/stale binary candidate.

**Architecture:** Refactor catalog discovery into a testable candidate collector and a candidate-by-candidate extraction pipeline. Discover the Codex desktop resource binary, `PATH`, application cache roots, and `~/.codex/bin`; deduplicate and sort candidates by modification time, then validate actual embedded catalog content before choosing one. Serialize scans, persist successful output atomically, retain a structurally valid last-known-good catalog, and expose failures through an asynchronous Tauri command so the frontend can show a warning while remaining responsive.

**Tech Stack:** Rust 2021, Tauri 2, React 19, TypeScript, Vitest, Cargo tests, `memchr`, `tempfile`.

---

### Task 1: Reproduce and fix Codex binary candidate discovery

**Files:**
- Modify: `crates/quotio-core/src/codex_catalog.rs:17-117`
- Test: `crates/quotio-core/src/codex_catalog.rs:287-374`

- [ ] **Step 1: Write failing tests for fresh WindowsApps and stale direct binaries**

Add test-only filesystem helpers and two tests using unique directories under `std::env::temp_dir()`:

```rust
fn temp_case(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "quotio-codex-catalog-{name}-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn discovers_windows_app_resource_without_local_bin_cache() {
    let dir = temp_case("windows-app-resource");
    let app = dir.join("app").join("Codex.exe");
    let cli = dir.join("app").join("resources").join("codex.exe");
    fs::create_dir_all(cli.parent().unwrap()).unwrap();
    fs::write(&app, b"launcher").unwrap();
    fs::write(&cli, b"cli").unwrap();

    let candidates = collect_codex_candidates(Some(&app), &[], &[], "codex.exe");

    assert!(candidates.contains(&cli));
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn newer_hashed_binary_is_not_hidden_by_old_direct_binary() {
    let dir = temp_case("newer-hashed");
    let direct = dir.join("codex.exe");
    let hashed = dir.join("new-version").join("codex.exe");
    fs::write(&direct, b"old").unwrap();
    std::thread::sleep(std::time::Duration::from_millis(1100));
    fs::create_dir_all(hashed.parent().unwrap()).unwrap();
    fs::write(&hashed, b"new").unwrap();

    let candidates = collect_codex_candidates(None, &[dir.clone()], &[], "codex.exe");

    assert_eq!(candidates.first(), Some(&hashed));
    assert!(candidates.contains(&direct));
    let _ = fs::remove_dir_all(dir);
}
```

- [ ] **Step 2: Run the focused tests and verify RED**

Run:

```powershell
cargo test -p quotio-core codex_catalog::tests::discovers_windows_app_resource_without_local_bin_cache -- --exact
cargo test -p quotio-core codex_catalog::tests::newer_hashed_binary_is_not_hidden_by_old_direct_binary -- --exact
```

Expected: compilation fails because `collect_codex_candidates` does not exist. This proves the tests exercise the missing discovery boundary.

- [ ] **Step 3: Implement candidate collection**

Add helpers with production inputs separated from filesystem logic:

```rust
fn collect_codex_candidates(
    app_path: Option<&Path>,
    roots: &[PathBuf],
    path_dirs: &[PathBuf],
    exe: &str,
) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(app) = app_path {
        if let Some(app_dir) = app.parent() {
            paths.push(app_dir.join("resources").join(exe));
            if let Some(contents) = app_dir.parent() {
                paths.push(contents.join("Resources").join(exe));
            }
        }
    }
    for dir in path_dirs {
        paths.push(dir.join(exe));
    }
    for root in roots {
        paths.push(root.join(exe));
        if let Ok(entries) = fs::read_dir(root) {
            for entry in entries.flatten() {
                paths.push(entry.path().join(exe));
            }
        }
    }
    paths.retain(|path| path.is_file());
    paths.sort_by(|left, right| candidate_mtime(right).cmp(&candidate_mtime(left)));
    deduplicate_paths(paths)
}
```

Build production roots from platform application-data paths, `std::env::split_paths(PATH)`, and `codex_launch::detect_codex_app_path_cached()`. Deduplicate using `fs::canonicalize()` when available and the original absolute path otherwise.

- [ ] **Step 4: Run focused tests and verify GREEN**

Run:

```powershell
cargo test -p quotio-core codex_catalog::tests::discovers_windows_app_resource_without_local_bin_cache -- --exact
cargo test -p quotio-core codex_catalog::tests::newer_hashed_binary_is_not_hidden_by_old_direct_binary -- --exact
```

Expected: both tests pass.

- [ ] **Step 5: Commit candidate discovery**

```powershell
git add crates/quotio-core/src/codex_catalog.rs
git commit -m "fix(codex): discover catalog binaries across install layouts"
```

### Task 2: Try every candidate and retain a last-known-good catalog

**Files:**
- Modify: `crates/quotio-core/Cargo.toml`
- Modify: `crates/quotio-core/src/codex_catalog.rs:22-285`
- Test: `crates/quotio-core/src/codex_catalog.rs`

- [ ] **Step 1: Add failing tests for invalid-first fallback and existing-catalog fallback**

Refactor the filesystem boundary toward a wished-for helper:

```rust
#[test]
fn extraction_continues_after_an_invalid_candidate() {
    let dir = temp_case("candidate-fallback");
    let invalid = dir.join("invalid.exe");
    let valid = dir.join("valid.exe");
    let target = dir.join("quotio-model-catalog.json");
    let meta = dir.join("quotio-model-catalog.meta.json");
    fs::write(&invalid, b"not a catalog").unwrap();
    fs::write(&valid, catalog_bytes(ONE_MODEL)).unwrap();

    let result = ensure_catalog_from(&[invalid, valid.clone()], &target, &meta).unwrap();

    assert_eq!(result, target);
    assert_eq!(fs::read_to_string(&target).unwrap(), ONE_MODEL);
    assert_eq!(fs::read_to_string(&meta).unwrap(), fingerprint(&valid).unwrap());
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn valid_existing_catalog_survives_when_all_candidates_fail() {
    let dir = temp_case("last-known-good");
    let invalid = dir.join("invalid.exe");
    let target = dir.join("quotio-model-catalog.json");
    let meta = dir.join("quotio-model-catalog.meta.json");
    fs::write(&invalid, b"not a catalog").unwrap();
    fs::write(&target, ONE_MODEL).unwrap();

    let result = ensure_catalog_from(&[invalid], &target, &meta).unwrap();

    assert_eq!(result, target);
    assert_eq!(fs::read_to_string(&target).unwrap(), ONE_MODEL);
    let _ = fs::remove_dir_all(dir);
}
```

- [ ] **Step 2: Run focused tests and verify RED**

Run:

```powershell
cargo test -p quotio-core codex_catalog::tests::extraction_continues_after_an_invalid_candidate -- --exact
cargo test -p quotio-core codex_catalog::tests::valid_existing_catalog_survives_when_all_candidates_fail -- --exact
```

Expected: compilation fails because `ensure_catalog_from` does not exist.

- [ ] **Step 3: Add atomic persistence dependency**

Add to `crates/quotio-core/Cargo.toml`:

```toml
tempfile = "3"
```

Use `tempfile::NamedTempFile::new_in(parent)`, `write_all`, `sync_all`, and `persist(target)` so a successful catalog replaces the destination atomically and a failed persist does not expose partial JSON.

- [ ] **Step 4: Implement validated cache and candidate fallback**

Introduce:

```rust
fn is_valid_catalog_text(text: &str) -> bool;

fn ensure_catalog_from(
    candidates: &[PathBuf],
    target: &Path,
    meta: &Path,
) -> Result<PathBuf, String>;

fn atomic_write(path: &Path, content: &[u8]) -> Result<(), String>;
```

`ensure_catalog_from` must:

1. Return the cached target only when its JSON validates and the metadata fingerprint matches one of the current candidates.
2. Iterate all candidates and continue after open, anchor, extraction, or validation failure.
3. Atomically persist the first valid extraction, then best-effort write metadata.
4. Return a valid existing target after all candidates fail.
5. Return an error containing the candidate count when neither extraction nor last-known-good data is available.

Wrap public execution in a process-wide mutex:

```rust
static CATALOG_SCAN_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
```

Recover a poisoned lock with `poisoned.into_inner()` so one panic cannot permanently disable catalog generation.

Keep compatibility wrappers:

```rust
pub fn ensure_catalog() -> Option<PathBuf> {
    ensure_catalog_result().ok()
}

pub fn reasoning_levels(model_slug: &str) -> Vec<String> {
    reasoning_levels_result(model_slug).unwrap_or_default()
}
```

Add `reasoning_levels_result` for the Tauri diagnostic path.

- [ ] **Step 5: Run focused and module tests**

Run:

```powershell
cargo test -p quotio-core codex_catalog -- --nocapture
```

Expected: all non-ignored catalog tests pass; the real-binary test remains ignored unless explicitly requested.

- [ ] **Step 6: Commit fallback and cache changes**

```powershell
git add crates/quotio-core/Cargo.toml Cargo.lock crates/quotio-core/src/codex_catalog.rs
git commit -m "fix(codex): validate and retain generated model catalogs"
```

### Task 3: Move scanning off the UI thread and surface loading failures

**Files:**
- Modify: `apps/desktop/src-tauri/src/lib.rs:139-144`
- Modify: `apps/desktop/src/components/sections/AgentsScreen.tsx:225-253`
- Modify: `apps/desktop/src/components/sections/AgentsScreen.tsx:618-625`
- Modify: `apps/desktop/src/dev/mockBackend.ts:716-738`
- Modify: `apps/desktop/src/i18n.tsx`
- Modify: `apps/desktop/src/App.css:5174-5211`

- [ ] **Step 1: Change the Tauri command to an async blocking task**

Implement:

```rust
#[tauri::command]
async fn fetch_codex_reasoning_levels(model: String) -> Result<Vec<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        quotio_core::codex_catalog::reasoning_levels_result(&model)
    })
    .await
    .map_err(|error| format!("读取 Codex 模型目录任务异常：{error}"))?
}
```

This keeps the existing command name while moving large file I/O to Tauri's blocking pool.

- [ ] **Step 2: Add frontend loading and failure state**

Add state beside `modelLevels`:

```tsx
const [modelLevelsLoading, setModelLevelsLoading] = useState(false);
const [modelLevelsError, setModelLevelsError] = useState("");
```

Before invoking, set loading and clear the error. On success, verify `Array.isArray(levels)` before storing it. On failure, keep the conservative fallback and store a localized warning. In `finally`, clear loading only when the request is not stale.

Render a short hint under the reasoning selector:

```tsx
{modelLevelsLoading ? (
  <span className="field-hint">{t("agents.reasoningLoading", "正在后台读取 Codex 模型目录…")}</span>
) : modelLevelsError ? (
  <span className="field-hint field-hint--warning">{modelLevelsError}</span>
) : null}
```

The select remains usable with the current/fallback values while loading.

Add compact `.codex-reasoning-status` and `.codex-reasoning-status.err` styles in `App.css`; keep them text-only so the loading message does not resize the form like the larger launch-status box.

- [ ] **Step 3: Add mock and translations**

Add a `fetch_codex_reasoning_levels` case to `mockInvoke` returning model-specific arrays, including `max` and `ultra` for `gpt-5.6-sol`. Add English and Chinese strings for loading and failure states.

- [ ] **Step 4: Run frontend tests and build**

Run:

```powershell
npm --prefix apps/desktop test -- --reporter=dot
npm run web:build
```

Expected: 12 existing tests pass and TypeScript/Vite production build exits zero.

- [ ] **Step 5: Commit asynchronous UI integration**

```powershell
git add apps/desktop/src-tauri/src/lib.rs apps/desktop/src/components/sections/AgentsScreen.tsx apps/desktop/src/dev/mockBackend.ts apps/desktop/src/i18n.tsx
git commit -m "fix(agents): load Codex reasoning levels without blocking"
```

### Task 4: Format and verify the complete fix

**Files:**
- Verify all modified files

- [ ] **Step 1: Format the new catalog module and inspect all diffs**

Run:

```powershell
rustfmt --edition 2021 crates/quotio-core/src/codex_catalog.rs
git diff --stat
git diff -- crates/quotio-core/src/codex_catalog.rs apps/desktop/src-tauri/src/lib.rs
```

Do not run repository-wide rewriting: the current branch already contains unrelated files that do not pass a full `cargo fmt --check`. Ensure the new module is formatted and the small Tauri command edit follows the surrounding style without reformatting the rest of `lib.rs`.

- [ ] **Step 2: Run fresh verification**

Run:

```powershell
cargo test --workspace
npm --prefix apps/desktop test -- --reporter=dot
npm run web:build
rustfmt --check --edition 2021 crates/quotio-core/src/codex_catalog.rs
git diff --check
```

Expected: all tests and builds exit zero; only the pre-existing dead-code warning may remain; the changed catalog module and whitespace checks report no differences.

- [ ] **Step 3: Inspect repository state and behavior evidence**

Run:

```powershell
git status --short --branch
git log --oneline --decorate -6
```

Confirm `.codex-vpn-check/` remains untracked and untouched, and all implementation commits are on `codex/fix-codex-catalog-discovery`.

- [ ] **Step 4: Commit any final formatting-only adjustment**

If formatting produced a remaining tracked diff:

```powershell
git add crates/quotio-core/src/codex_catalog.rs apps/desktop/src-tauri/src/lib.rs
git commit -m "style: format Codex catalog changes"
```

Do not create an empty commit when no formatting diff remains.
