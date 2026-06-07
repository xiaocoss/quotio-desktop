# Quotio Desktop Migration

## Current project

```text
D:\项目\quotio-desktop
```

Independent Tauri-based cross-platform project. Continue new implementation here.

## Reference project

```text
D:\项目\quotio-master
```

Original macOS SwiftUI implementation. Use it as behavior, data-contract, proxy-resource, and screen-flow reference only.

Key reference areas:

```text
D:\项目\quotio-master\Quotio\Models
D:\项目\quotio-master\Quotio\Services
D:\项目\quotio-master\Quotio\Views
D:\项目\quotio-master\Quotio\Resources\Proxy
```

## Current architecture

```text
apps/desktop            React + Tauri UI shell
apps/desktop/src-tauri  Tauri command bridge and desktop shell
crates/quotio-types     Shared contracts
crates/quotio-core      App state, proxy lifecycle, Management API, agent/fallback runtime
crates/quotio-platform  OS adapters: paths, CLI search, backups, credentials, open path
resources/proxy         Platform-specific CLIProxyAPI binaries
```

## Completed

- Independent Tauri workspace under `D:\项目\quotio-desktop`.
- Shared Rust contracts and matching frontend contracts.
- Proxy lifecycle core:
  - Platform resource lookup
  - Managed binary copy
  - Config generation
  - Start / stop / restart paths
  - Health check
  - Missing binary / crashed / error state projection
  - Runtime proxy resource diagnostics
  - Tauri packaged resource directory injection
- Management API client and Tauri bridge:
  - Auth files
  - OAuth URL / polling
  - Usage and quota snapshot
  - Logs
  - API keys
  - Routing / debug / request log / proxy URL / retry controls
  - Vertex service account import
  - Remote Management snapshot refresh
  - Optional `/usage` and `/logs` degradation when external proxy omits or rejects those segments
  - `/auth-files` compatibility for integer count, null, array, and object variants
  - Chunked HTTP response decoding
- Core React UI:
  - Dashboard
  - Providers
  - Quota
  - Logs
  - Settings
  - API Keys
  - Agents
  - Fallback
- Secondary runtime UI and bridge:
  - Agents detection refresh
  - Agent configuration read
  - Agent manual config output
  - Agent automatic config write
  - Agent backup list / restore
  - Agent reset to default
  - Fallback virtual model config
  - Fallback route-cache state display
  - Fallback model discovery with built-in fallback list
  - Remote connection status placement in Settings
- Platform adapters:
  - CLI path search across PATH and common npm/cargo/bun/deno/volta/asdf/mise/nvm/fnm locations
  - Safe write with backup
  - Backup list / restore
  - Credential storage through `keyring`
  - Config directory open action
  - Tauri autostart plugin bridge
  - Tauri notification plugin bridge and frontend permission/test-notification flow
- Management key handling:
  - Local management key generated and stored through platform credential storage when available
  - Remote management key is migrated to credential storage on save/startup
  - `AppState` no longer returns remote management key plaintext
  - Settings file persists without remote management key plaintext
  - Windows Credential Manager target is `remote-management-key.quotio`
- Proxy resource migration:
  - Copied reference macOS/darwin proxy binary from:

```text
D:\项目\quotio-master\Quotio\Resources\Proxy\cli-proxy-api-plus
```

  - Current target:

```text
D:\项目\quotio-desktop\resources\proxy\darwin\cli-proxy-api-plus
```

- SwiftUI visual alignment:
  - First-pass macOS SwiftUI-style light desktop skin applied to sidebar, cards, buttons, pills, forms, diagnostics, and dark-mode override.
- Tauri resource declaration includes `resources/proxy/**/*`.
- Windows packaging completed for the current build:

```text
target/release/quotio-desktop.exe
target/release/bundle/msi/Quotio_0.1.0_x64_en-US.msi
target/release/bundle/nsis/Quotio_0.1.0_x64-setup.exe
```

## Current limitations

### Windows/Linux proxy binaries are still missing

Current resource state:

```text
resources/proxy/darwin/cli-proxy-api-plus        present
resources/proxy/windows/README.md                placeholder only
resources/proxy/windows/cli-proxy-api-plus.exe   missing
resources/proxy/linux/README.md                  placeholder only
resources/proxy/linux/cli-proxy-api-plus         missing
```

Windows/Linux local hosted proxy runtime remains in `missing_binary` state until real binaries are provided.

### Local hosted proxy runtime is not fully verified

Remote Management against the external proxy at `http://127.0.0.1:8317` is verified. Local hosted proxy startup is still blocked by the missing Windows/Linux binaries.

Still requiring local runtime validation:

- Managed process start / stop / restart / health check
- Local generated management key against the managed proxy
- Local ManagementSnapshot refresh
- Local API key / auth files / logs / fallback write operations
- Crash and recovery projection

### Real proxy request bridge is not fully reproduced

The reference macOS project performs request-level fallback behavior through `ProxyBridge.swift` / proxy runtime behavior.

Current Tauri project covers:

- Local fallback configuration
- Route-cache derived state
- Model discovery fallback list
- Proxy process lifecycle and Management API controls

Still requiring real proxy/runtime validation:

- Actual request replacement to fallback entries
- Runtime route state reported by live proxy bridge
- Real `/v1/models` or provider-backed model discovery beyond the current built-in fallback list

### Management API transport

Current Rust client is plain HTTP/1.1 oriented over `TcpStream`. Remote HTTPS support has not been proven.

### Cross-platform runtime regression is pending

Windows build and packaging have passed. macOS/Linux build, installation, tray, autostart, notification, and proxy runtime regression remain pending.

## Latest verified commands

Run from:

```text
D:\项目\quotio-desktop
```

```bash
cargo fmt --all
cargo check --workspace
cargo test -p quotio-core
cargo test -p quotio-core management
npm --prefix apps/desktop run build
npm --prefix apps/desktop run tauri build
```

Latest result:

```text
cargo fmt --all: passed
cargo check --workspace: passed
cargo test -p quotio-core: 13 passed
cargo test -p quotio-core management: 9 passed
npm --prefix apps/desktop run build: passed, 47 modules transformed
npm --prefix apps/desktop run tauri build: passed
Windows release exe: generated
Windows MSI installer: generated
Windows NSIS installer: generated
```

## Remaining work

### 1. Local hosted proxy runtime validation

Needs real platform binaries and runtime tests:

- Add real Windows `resources/proxy/windows/cli-proxy-api-plus.exe`
- Add real Linux `resources/proxy/linux/cli-proxy-api-plus`
- Validate darwin `cli-proxy-api-plus` launch on macOS
- Validate `missing_binary / stopped / running / crashed / error`
- Validate generated local Management API bearer auth against real managed proxy
- Validate health check and snapshot refresh
- Validate local API key / auth files / logs / fallback operations end-to-end

### 2. Real fallback bridge/model discovery

Current fallback state is local/config-derived. Remaining runtime work:

- Pull real model list from Management API or proxy `/v1/models`
- Feed live fallback route decisions from proxy/runtime
- Verify actual request replacement behavior end-to-end

### 3. Packaging and release regression

Partially complete on Windows. Still pending:

- Windows installer install/uninstall flow
- Windows tray open/quit/reopen behavior
- macOS `.app` / `.dmg`
- Linux AppImage/deb/rpm
- Notifications and launch-at-login platform behavior
- Tauri updater / release flow

## Functional Verification Playbook

Use this section as the source of truth for completeness testing. Each check should produce one of these statuses:

```text
pass        behavior verified and reproducible
failed      behavior tested and failed; keep exact error/evidence
blocked     cannot test because dependency/tool/runtime is missing
not_tested  not run yet
out_of_scope intentionally excluded from current scope
```

### Verification baseline

| Area | Check | Command / action | Expected result | Current status | Evidence |
|---|---|---|---|---|---|
| Frontend build | Production web build | `npm --prefix apps/desktop run build` | TypeScript and Vite build pass | pass | Latest run transformed 47 modules |
| Rust format | Workspace formatting | `cargo fmt --all` | No formatting diff / command exits 0 | pass | Latest run passed |
| Rust compile | Workspace compile | `cargo check --workspace` | All crates compile | pass | Latest run passed |
| Rust tests | Core package | `cargo test -p quotio-core` | Core tests pass | pass | Latest run: 13 passed |
| Rust tests | Core management | `cargo test -p quotio-core management` | Management API tests pass | pass | Latest run: 9 passed |
| Tauri packaging | Windows app package | `npm --prefix apps/desktop run tauri build` | Windows app and installers are generated | pass | release exe, MSI, and NSIS generated |

### Proxy runtime verification

| Area | Preconditions | Verification steps | Expected result | Current status | Evidence / notes |
|---|---|---|---|---|---|
| Resource lookup: darwin | Run on macOS or inspect resource directory | Confirm `resources/proxy/darwin/cli-proxy-api-plus` exists | Darwin resource is present | pass | File copied from reference project |
| Resource lookup: Windows | Run on Windows without proxy exe | Start/check proxy state | State reports `missing_binary`; expected path points to Windows resource dir | pass | App reported `MISSING_BINARY` and Windows resource dir; `resources/proxy/windows` only has README |
| Resource lookup: Linux | Run on Linux without proxy binary | Start/check proxy state | State reports `missing_binary`; expected path points to Linux resource dir | blocked | Needs Linux host validation; Linux binary is missing |
| Remote Management auth | External proxy at `http://127.0.0.1:8317` with correct key | Refresh Management snapshot in Remote mode | Bearer auth succeeds; snapshot fields update | pass | Dashboard showed real auth files, API keys, and provider state |
| Remote Management compatibility | External proxy returns optional/mismatched segments | Refresh Management snapshot | Missing `/usage`, failing `/logs`, integer `/auth-files`, and chunked body do not break refresh | pass | Compatibility tests and runtime refresh passed |
| Config generation | Valid local proxy binary available | Start managed proxy | Config file is written under app config root; auth dir exists | blocked | Requires real Windows/Linux proxy binary |
| Process lifecycle | Valid local proxy binary available | Start, stop, restart proxy from Dashboard | State transitions through starting/running/stopping/stopped | blocked | Requires real Windows/Linux proxy binary |
| Health check | Local proxy running | Trigger health check | Health state becomes healthy or reports clear failure | blocked | Requires real Windows/Linux proxy binary |
| Crash projection | Local proxy exits after launch | Start proxy with crashing binary/config | State reports crashed/error with exit code and crash count | blocked | Requires real Windows/Linux proxy binary |
| Local Management auth | Managed proxy running with generated management key | Refresh local Management snapshot | Bearer auth succeeds; snapshot fields update | blocked | Requires real Windows/Linux proxy binary |

### Management key and credential verification

| Area | Preconditions | Verification steps | Expected result | Current status | Evidence / notes |
|---|---|---|---|---|---|
| Local management key | Clean credential store or first app start | Start app and inspect Settings credential state | Local key configured; no plaintext key exposed to React | not_tested | Local hosted proxy still blocked by missing binary |
| Remote key save | Remote key entered in Settings | Save settings, refresh state | Remote key marked configured/masked; input clears plaintext draft | pass | Windows Credential Manager target `remote-management-key.quotio`; key is not echoed to frontend |
| Settings plaintext guard | Remote key has been saved | Inspect settings file under app config root | `remote_management_key` is absent or null | pass | `C:\Users\lilin\AppData\Roaming\Quotio\settings.json` keeps remote endpoint and mode without plaintext key |
| Remote key runtime use | Remote key stored in credential manager | Refresh Remote Management snapshot | Stored key is used for bearer auth | pass | 401 resolved after correct plaintext key was saved; snapshot refreshed |
| Remote key migration | Existing settings file contains plaintext key | Start app | Key is migrated into credential storage and settings file is rewritten without plaintext | not_tested | Requires seeded legacy settings file |
| Remote key clear | Remote key already stored | Click clear remote key | Credential status reports remote key missing | not_tested | Requires running UI flow |
| Credential unavailable state | Keyring backend unavailable | Start app / save key | UI reports credential risk instead of silently succeeding | not_tested | Requires platform-specific negative test |

### Agent verification

| Area | Preconditions | Verification steps | Expected result | Current status | Evidence / notes |
|---|---|---|---|---|---|
| Detection refresh | One or more CLI agents installed or absent | Open Agents and click re-detect | Installed/configured state reflects host binaries and config files | not_tested | Needs host CLI matrix |
| Manual output | Select each supported agent | Generate manual config | Raw config output includes target path/instructions and selected models | not_tested | Frontend build passed; runtime not manually verified |
| Automatic write | Use disposable HOME/config root | Write proxy config for each agent | Target config is updated; user keys outside managed area are preserved | not_tested | Needs sandboxed config fixtures |
| Backup creation | Existing target config present | Automatic write | Backup appears in backup list | not_tested | Needs sandboxed config fixtures |
| Backup restore | Backup exists | Restore backup | Target config returns to backed-up content; current file is backed up first | not_tested | Needs sandboxed config fixtures |
| Reset default | Agent has proxy config | Reset configuration | Quotio/CLIProxyAPI managed config is removed or default config restored without destructive unrelated changes | not_tested | Needs per-agent fixture validation |
| Unsupported/missing agent | Agent binary absent | Read/configure missing agent | UI shows clear state/error; app does not crash | not_tested | Needs runtime UI check |

Supported agent matrix to validate:

| Agent | Detection | Manual output | Automatic proxy write | Backup/restore | Reset default | Status |
|---|---|---|---|---|---|---|
| Claude Code | not_tested | not_tested | not_tested | not_tested | not_tested | pending runtime validation |
| Codex | not_tested | not_tested | not_tested | not_tested | not_tested | pending runtime validation |
| Gemini CLI | not_tested | not_tested | not_tested | not_tested | not_tested | pending runtime validation |
| Amp | not_tested | not_tested | not_tested | not_tested | not_tested | pending runtime validation |
| OpenCode | not_tested | not_tested | not_tested | not_tested | not_tested | pending runtime validation |
| Factory Droid | not_tested | not_tested | not_tested | not_tested | not_tested | pending runtime validation |

### Fallback verification

| Area | Preconditions | Verification steps | Expected result | Current status | Evidence / notes |
|---|---|---|---|---|---|
| Global switch | Open Fallback page | Toggle fallback enabled/disabled | State persists; disabled mode prevents effective route state | not_tested | Runtime UI flow not manually verified |
| Route cache switch | Fallback enabled | Toggle route caching | Route state appears when enabled and clears when disabled | not_tested | Runtime UI flow not manually verified |
| Virtual model CRUD | Fallback enabled | Add, rename, toggle, delete virtual model | UI and local config update consistently | not_tested | Runtime UI flow not manually verified |
| Entry CRUD | Provider list available | Add, move, remove fallback entries | Priority order updates and persists | not_tested | Runtime UI flow not manually verified |
| Derived route state | Enabled virtual model has entries | Refresh route state | Current entry and total entries are shown | not_tested | Config-derived state implemented; runtime UI flow not manually verified |
| Model discovery fallback | No real model source available | Click discover models | Built-in fallback model list appears; status explains fallback source | not_tested | Runtime UI flow not manually verified |
| Real model discovery | Proxy/Management API can list models | Click discover models | Models come from live API rather than built-in fallback | blocked | Requires real local proxy/runtime source validation |
| Request-level fallback | Real proxy bridge supports fallback | Send request through virtual model | Failed primary route switches to fallback entry | blocked | Requires real proxy bridge/runtime support |

### Platform adapter verification

| Area | Preconditions | Verification steps | Expected result | Current status | Evidence / notes |
|---|---|---|---|---|---|
| Open config directory | Tauri app running | Click open config directory | File manager opens config root | not_tested | Command and UI bridge implemented |
| Launch at login | Platform supports autostart | Toggle launch at login | System autostart state changes and Settings reflects it | not_tested | Plugin bridge implemented |
| Notification permission | Desktop notification support | Enable notifications | Permission is requested; failure is visible | not_tested | Frontend plugin flow implemented |
| Test notification | Notification permission granted | Click test notification | Native notification appears | not_tested | Frontend plugin flow implemented |
| CLI search | Agents installed in common dirs | Re-detect agents | PATH and common version-manager dirs are searched | not_tested | Needs host matrix |
| Backup safety | Existing sensitive config file | Automatic write / restore | Backup is created; sensitive file permissions are preserved where supported | not_tested | Needs platform fixture |

### UI completeness verification

| Page | Required flow | Expected result | Current status | Evidence / notes |
|---|---|---|---|---|
| Dashboard | Remote Management refresh | State cards update without reload | pass | Remote snapshot showed auth files, API keys, and provider state |
| Dashboard | Local proxy actions and health | Managed proxy state updates | blocked | Requires Windows/Linux proxy binary |
| Providers | Snapshot read for auth files/API keys/providers | Remote Management data is visible | pass | Remote snapshot displayed real counts and provider state |
| Providers | OAuth/API key/auth file write actions | Management snapshot refreshes after action | not_tested | Requires targeted runtime write-flow validation |
| Quota | Usage and quota controls | Usage/config state reflects Management API | not_tested | `/usage` is optional for external proxy; local runtime still blocked |
| Logs | Request-log toggle, clear logs, filtering | Logs update and UI remains responsive | not_tested | `/logs` is optional for external proxy; local runtime still blocked |
| API Keys | Add/update/delete API keys | Management key list updates | not_tested | Requires targeted runtime write-flow validation |
| Agents | Detect/read/configure/backup/restore/reset | Agent state and result panels update | not_tested | Runtime UI flow not manually verified |
| Fallback | Configure virtual models/routes/discovery | Local config and runtime cards update | not_tested | Runtime UI flow not manually verified |
| Settings | Remote endpoint/key state and resource diagnostics | Settings show masked credential state and proxy resource status | pass | Remote endpoint `http://127.0.0.1:8317`; Windows resource status showed missing binary |
| Settings | Autostart/notification/open-directory actions | Platform actions work from Settings | not_tested | Requires targeted platform UI validation |

### Cross-platform verification matrix

| Platform | Frontend build | Rust check | Proxy binary | Proxy runtime | Credential storage | Autostart | Notification | Status |
|---|---|---|---|---|---|---|---|---|
| Windows | pass | pass | missing local binary | remote pass / local blocked | remote key pass | not_tested | not_tested | build/package pass; Remote Management pass; local hosted proxy blocked by Windows binary |
| macOS / darwin | not_tested on host | not_tested on host | present | not_tested | not_tested | not_tested | not_tested | needs macOS runtime validation |
| Linux | not_tested | not_tested | missing | blocked | not_tested | not_tested | not_tested | blocked by Linux proxy binary |

### Evidence capture rules

For every failed or passed runtime check, record:

- Platform and architecture
- Exact command or UI path
- Input values used
- App state before/after
- Generated file path, if any
- Relevant log line or error text
- Whether the test used clean config state or existing user config

Do not mark a feature `pass` unless it is reproducible from a clean or explicitly described baseline.

## Startup prompt for future chat

```text
当前项目：D:\项目\quotio-desktop
参考项目：D:\项目\quotio-master

继续在 quotio-desktop 做跨平台实现，不改 quotio-master。

已完成核心功能：Dashboard / Providers / Quota / Logs / Settings / Agents / Fallback / API Keys 页面；Management API Rust client 和 Tauri 命令桥；Remote Management 外部代理闭环；usage/logs 可选段降级；auth-files 差异结构兼容；chunked HTTP 响应解码；Management key 安全存储；proxy 资源诊断；打包态资源路径注入；SwiftUI 风格第一轮视觉重皮肤。

已验证：cargo fmt --all、cargo check --workspace、cargo test -p quotio-core、cargo test -p quotio-core management、npm --prefix apps/desktop run build、npm --prefix apps/desktop run tauri build 均通过。Windows release exe、MSI、NSIS 已生成。Remote Management 已在 http://127.0.0.1:8317 跑通，Dashboard 能刷新真实快照。

当前阻塞：resources/proxy/windows/cli-proxy-api-plus.exe 和 resources/proxy/linux/cli-proxy-api-plus 缺失；本地托管代理 start/stop/health/ManagementSnapshot/API key/auth/logs/fallback 端到端仍未验证；托盘、安装包安装流、退出重开、macOS/Linux 跨平台回归仍待做。
```