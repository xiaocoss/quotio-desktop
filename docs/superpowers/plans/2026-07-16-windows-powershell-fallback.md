# Windows PowerShell Fallback Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make Quotio v0.7.1 launch Windows Store Codex when `powershell.exe` is missing from the inherited `PATH`.

**Architecture:** Centralize Windows PowerShell discovery and execution in `codex_launch.rs`. Try ordered absolute Windows PowerShell and PowerShell 7 paths plus PATH names, retry only `ErrorKind::NotFound`, and reuse the selected interpreter for Appx detection, Store activation, cleanup, PID probing, and CLI launch.

**Tech Stack:** Rust 2021, `std::process::Command`, Windows PowerShell 5.1 / PowerShell 7, Tauri 2, Cargo, npm/Vite.

---

### Task 1: Add candidate discovery and fallback tests

**Files:**
- Modify: `crates/quotio-core/src/codex_launch.rs:5-31`
- Test: `crates/quotio-core/src/codex_launch.rs:1030-1090`

- [ ] **Step 1: Add failing Windows tests**

Add to the existing test module:

```rust
#[cfg(target_os = "windows")]
#[test]
fn powershell_candidates_prefer_system_paths_and_include_pwsh() {
    let root = temp_codex_home("ql_powershell_candidates_system");
    let system_root = root.join("Windows");
    let program_files = root.join("Program Files");
    let system32 = system_root.join("System32/WindowsPowerShell/v1.0/powershell.exe");
    let sysnative = system_root.join("Sysnative/WindowsPowerShell/v1.0/powershell.exe");
    let pwsh = program_files.join("PowerShell/7/pwsh.exe");
    for path in [&system32, &sysnative, &pwsh] {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, b"").unwrap();
    }
    assert_eq!(
        powershell_candidates_from_roots(Some(&system_root), Some(&program_files)),
        vec![
            system32,
            sysnative,
            PathBuf::from("powershell.exe"),
            pwsh,
            PathBuf::from("pwsh.exe"),
        ]
    );
    let _ = std::fs::remove_dir_all(root);
}

#[cfg(target_os = "windows")]
#[test]
fn powershell_candidates_fall_back_to_path_names_when_absolute_files_are_missing() {
    assert_eq!(
        powershell_candidates_from_roots(None, None),
        vec![PathBuf::from("powershell.exe"), PathBuf::from("pwsh.exe")]
    );
}

#[cfg(target_os = "windows")]
#[test]
fn powershell_candidates_are_case_insensitively_deduplicated() {
    let mut candidates = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    push_unique_powershell_candidate(
        &mut candidates,
        &mut seen,
        PathBuf::from("PowerShell.exe"),
    );
    push_unique_powershell_candidate(
        &mut candidates,
        &mut seen,
        PathBuf::from("powershell.exe"),
    );
    assert_eq!(candidates, vec![PathBuf::from("PowerShell.exe")]);
}

#[cfg(target_os = "windows")]
#[test]
fn powershell_candidate_runner_retries_only_not_found() {
    use std::io::{Error, ErrorKind};
    let candidates = vec![PathBuf::from("missing.exe"), PathBuf::from("available.exe")];
    let mut attempted = Vec::new();
    let (program, value) = try_powershell_candidates(&candidates, |candidate| {
        attempted.push(candidate.to_path_buf());
        if candidate == Path::new("missing.exe") {
            Err(Error::new(ErrorKind::NotFound, "missing"))
        } else {
            Ok(42)
        }
    })
    .unwrap();
    assert_eq!(program, PathBuf::from("available.exe"));
    assert_eq!(value, 42);
    assert_eq!(attempted, candidates);
}

#[cfg(target_os = "windows")]
#[test]
fn powershell_candidate_runner_preserves_other_errors() {
    use std::io::{Error, ErrorKind};
    let candidates = vec![PathBuf::from("denied.exe"), PathBuf::from("unused.exe")];
    let mut attempted = Vec::new();
    let error = try_powershell_candidates(&candidates, |candidate| -> std::io::Result<()> {
        attempted.push(candidate.to_path_buf());
        Err(Error::new(ErrorKind::PermissionDenied, "denied"))
    })
    .unwrap_err();
    assert_eq!(attempted, vec![PathBuf::from("denied.exe")]);
    assert!(error.contains("denied.exe"));
}
```

- [ ] **Step 2: Verify the tests fail**

Run: `cargo test -p quotio-core powershell_candidate -- --nocapture`

Expected: compilation fails because the candidate helpers are missing.

- [ ] **Step 3: Implement candidate discovery and retry**

Generalize `quiet_command` and add these helpers near the top of `codex_launch.rs`:

```rust
use std::ffi::OsStr;

fn quiet_command(program: impl AsRef<OsStr>) -> Command {
    let mut command = Command::new(program);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
}

#[cfg(target_os = "windows")]
fn push_unique_powershell_candidate(
    candidates: &mut Vec<PathBuf>,
    seen: &mut std::collections::BTreeSet<String>,
    candidate: PathBuf,
) {
    let key = candidate.to_string_lossy().replace('/', "\\").to_lowercase();
    if seen.insert(key) {
        candidates.push(candidate);
    }
}

#[cfg(target_os = "windows")]
fn powershell_candidates_from_roots(
    system_root: Option<&Path>,
    program_files: Option<&Path>,
) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    if let Some(root) = system_root {
        for directory in ["System32", "Sysnative"] {
            let candidate = root
                .join(directory)
                .join("WindowsPowerShell/v1.0/powershell.exe");
            if candidate.is_file() {
                push_unique_powershell_candidate(&mut candidates, &mut seen, candidate);
            }
        }
    }
    push_unique_powershell_candidate(&mut candidates, &mut seen, PathBuf::from("powershell.exe"));
    if let Some(root) = program_files {
        let candidate = root.join("PowerShell/7/pwsh.exe");
        if candidate.is_file() {
            push_unique_powershell_candidate(&mut candidates, &mut seen, candidate);
        }
    }
    push_unique_powershell_candidate(&mut candidates, &mut seen, PathBuf::from("pwsh.exe"));
    candidates
}

#[cfg(target_os = "windows")]
fn windows_powershell_candidates() -> Vec<PathBuf> {
    let system_root = std::env::var_os("SystemRoot")
        .or_else(|| std::env::var_os("WINDIR"))
        .map(PathBuf::from);
    let program_files = std::env::var_os("ProgramFiles").map(PathBuf::from);
    powershell_candidates_from_roots(system_root.as_deref(), program_files.as_deref())
}

#[cfg(target_os = "windows")]
fn try_powershell_candidates<T, F>(
    candidates: &[PathBuf],
    mut execute: F,
) -> Result<(PathBuf, T), String>
where
    F: FnMut(&Path) -> std::io::Result<T>,
{
    let mut attempted = Vec::new();
    for candidate in candidates {
        attempted.push(candidate.display().to_string());
        match execute(candidate) {
            Ok(value) => return Ok((candidate.clone(), value)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                return Err(format!(
                    "调用 PowerShell 失败（{}）: {}",
                    candidate.display(), error
                ));
            }
        }
    }
    Err(format!(
        "未找到可用的 PowerShell（已尝试：{}）。请启用 Windows PowerShell 或安装 PowerShell 7。",
        attempted.join("、")
    ))
}
```

- [ ] **Step 4: Run focused tests**

Run: `cargo test -p quotio-core powershell_candidate -- --nocapture`

Expected: all five new tests pass.

- [ ] **Step 5: Commit**

```powershell
git add crates/quotio-core/src/codex_launch.rs
git commit -m "fix(codex): resolve PowerShell without relying on PATH"
```

### Task 2: Route all Codex PowerShell calls through the helper

**Files:**
- Modify: `crates/quotio-core/src/codex_launch.rs:135-165`
- Modify: `crates/quotio-core/src/codex_launch.rs:690-720`
- Modify: `crates/quotio-core/src/codex_launch.rs:782-930`

- [ ] **Step 1: Replace `run_powershell` with a selected-program runner**

```rust
#[cfg(target_os = "windows")]
fn run_powershell_with_program(
    script: &str,
) -> Result<(PathBuf, std::process::Output), String> {
    try_powershell_candidates(&windows_powershell_candidates(), |program| {
        quiet_command(program)
            .args(["-NoProfile", "-NonInteractive", "-Command", script])
            .output()
    })
}

#[cfg(target_os = "windows")]
fn run_powershell(script: &str) -> Result<std::process::Output, String> {
    run_powershell_with_program(script).map(|(_, output)| output)
}

#[cfg(target_os = "windows")]
fn resolve_powershell_program() -> Result<PathBuf, String> {
    run_powershell_with_program("$null").map(|(program, _)| program)
}
```

Keep `run_powershell_expect_success` unchanged.

- [ ] **Step 2: Migrate Appx detection and cleanup**

Use `run_powershell(...)` in `detect_codex_via_appx` and in the Node-process cleanup inside `close_codex_app`; remove both direct `quiet_command("powershell")` calls.

- [ ] **Step 3: Migrate CLI terminal launch**

Use the actual resolved program:

```rust
let powershell = resolve_powershell_program()?;
if let Ok(child) = Command::new("wt")
    .arg(powershell.as_os_str())
    .args(["-NoExit", "-Command", &codex_cmd])
    .spawn()
{
    return Ok(Some(child.id()));
}
let powershell = powershell.to_string_lossy().to_string();
Command::new("cmd")
    .args([
        "/d", "/s", "/c", "start", "", &powershell,
        "-NoExit", "-Command", &codex_cmd,
    ])
    .spawn()
    .map_err(|e| format!("打开终端失败: {e}"))?;
Ok(None)
```

- [ ] **Step 4: Verify hard-coded calls are gone**

Run:

```powershell
rg -n 'quiet_command\("powershell"\)|Command::new\("powershell"\)' crates/quotio-core/src/codex_launch.rs
```

Expected: no matches.

- [ ] **Step 5: Run Codex and core tests**

```powershell
cargo test -p quotio-core codex_launch::tests -- --nocapture
cargo test -p quotio-core
```

Expected: all non-ignored tests pass.

- [ ] **Step 6: Commit**

```powershell
git add crates/quotio-core/src/codex_launch.rs
git commit -m "fix(codex): reuse PowerShell fallback across Windows launch paths"
```

### Task 3: Prepare v0.7.1 metadata and run release verification

**Files:**
- Modify: `package.json`
- Modify: `apps/desktop/package.json`
- Modify: `apps/desktop/src-tauri/Cargo.toml`
- Modify: `apps/desktop/src-tauri/tauri.conf.json`
- Modify: `Cargo.lock`
- Modify: `README.md`
- Modify: `CHANGELOG.md`

- [ ] **Step 1: Bump the version**

Run: `npm run version:set -- 0.7.1`

Expected: four application version files and README change from 0.7.0 to 0.7.1.

- [ ] **Step 2: Add this changelog section above v0.7.0**

```markdown
## v0.7.1 - 2026-07-16

修复部分 Windows 机器启动商店版 Codex 时提示 `调用 PowerShell 失败: program not found` 的问题。

### 修复

- **Codex 的 Windows PowerShell 调用不再依赖 `PATH`**：优先使用系统 Windows PowerShell 的绝对路径，找不到时回退到 `powershell.exe` 和 PowerShell 7 的 `pwsh.exe`，兼容精简系统、环境变量缺失以及只安装 PowerShell 7 的机器。
- **统一修复 Appx 探测、商店版 AppsFolder 激活、进程清理、PID 探测和 CLI 终端启动**，避免只修启动按钮、其它路径仍然报同样错误。
- PowerShell 全部缺失时返回包含已尝试路径的明确提示；脚本自身失败时仍保留真实 stderr，不会被回退逻辑掩盖。
```

- [ ] **Step 3: Run verification**

```powershell
cargo test -p quotio-core
cargo check --workspace
npm --prefix apps/desktop run test
npm --prefix apps/desktop run build
git diff --check
```

Expected: tests and builds pass. Existing Vite chunk-size and Rust dead-code warnings are acceptable.

- [ ] **Step 4: Commit release metadata**

```powershell
git add -u
git commit -m "release: v0.7.1 — fix PowerShell path compatibility"
```

Expected: the untracked `设计图/设计图.lnk` is not staged.

### Task 4: Tag, push, and verify the GitHub Release

**Files:**
- Read: `.github/workflows/release.yml`
- External output: Git tag and GitHub Release `v0.7.1`

- [ ] **Step 1: Create and push the tag**

```powershell
git tag -a v0.7.1 -m "Quotio v0.7.1"
git push origin main v0.7.1
```

Expected: main and the new tag reach `origin`.

- [ ] **Step 2: Monitor the workflow**

```powershell
$runId = gh run list --repo xiaocoss/quotio-desktop --workflow release.yml --limit 10 --json databaseId,headBranch | ConvertFrom-Json |
  Where-Object { $_.headBranch -eq 'v0.7.1' } |
  Select-Object -First 1 -ExpandProperty databaseId
if (-not $runId) { throw 'v0.7.1 release workflow run not found' }
gh run view $runId --repo xiaocoss/quotio-desktop --json status,conclusion,jobs
```

Expected: Windows, macOS, Linux, and `rewrite-manifest` all complete with `success`.

- [ ] **Step 3: Verify the Release**

```powershell
gh release view v0.7.1 --repo xiaocoss/quotio-desktop --json name,tagName,url,body,assets,isDraft,isPrerelease
```

Expected: the Release is public, its body starts with the v0.7.1 changelog, and assets include Windows MSI/NSIS, macOS DMG, Linux AppImage/DEB/RPM, signatures, and `latest.json`.
