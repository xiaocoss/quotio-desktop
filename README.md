# Quotio Desktop

Quotio Desktop is the independent cross-platform migration project for Quotio.

The original `quotio-master` SwiftUI project remains the macOS reference implementation. This project is the new Tauri-based desktop app intended to support macOS, Windows, and Linux from one codebase.

Migration state and reference-project mapping are tracked in [`docs/MIGRATION.md`](docs/MIGRATION.md).

## Screenshots

<p align="center">
  <img src="assets/dashboard.png" width="820" alt="Quotio usage dashboard"><br>
  <sub><b>仪表盘 Dashboard</b> — calls, success rate, token usage and estimated cost by account / provider / model</sub>
</p>

<p align="center">
  <img src="assets/quota.png" width="820" alt="Quotio quota screen"><br>
  <sub><b>额度 Quota</b> — per-account Session (5h) / Weekly windows, plan, expiry, and Codex 主动重置次数 + reset</sub>
</p>

<details>
<summary>More screens · 更多界面（服务商 / 智能体 / 日志 / 设置）</summary>

<p align="center">
  <img src="assets/providers.png" width="820" alt="Providers"><br>
  <sub><b>服务商 Providers</b> — multi-account OAuth pool管理</sub>
</p>
<p align="center">
  <img src="assets/agents.png" width="820" alt="Agents"><br>
  <sub><b>智能体 Agents</b> — point CLI tools (Claude Code / Codex / Gemini …) at CLIProxyAPI</sub>
</p>
<p align="center">
  <img src="assets/logs.png" width="820" alt="Logs"><br>
  <sub><b>日志 Logs</b> — per-request history with tokens, latency and status</sub>
</p>
<p align="center">
  <img src="assets/settings.png" width="820" alt="Settings"><br>
  <sub><b>设置 Settings</b> — app mode, proxy, language, theme, privacy</sub>
</p>

</details>

## Structure

```text
apps/
└── desktop/              Tauri + React desktop application

crates/
├── quotio-types/         Shared Provider, account, quota, log, and settings models
├── quotio-core/          Cross-platform proxy, state, and Management API domain layer
└── quotio-platform/      OS-specific adapters for tray, startup, notifications, credentials, and paths

resources/
└── proxy/
    ├── darwin/           macOS CLIProxyAPI binary placeholder
    ├── windows/          Windows CLIProxyAPI binary placeholder
    └── linux/            Linux CLIProxyAPI binary placeholder
```

## Commands

```bash
npm --prefix apps/desktop install
npm --prefix apps/desktop run tauri dev
npm --prefix apps/desktop run tauri build
cargo check --workspace
```

Or use root shortcuts:

```bash
npm run desktop:install
npm run desktop:dev
npm run desktop:build
npm run cargo:check
```

## Release

```bash
# bump the version across all four manifests (tauri.conf.json, Cargo.toml, both package.json)
npm run version:set -- minor   # major | minor | patch | X.Y.Z

# build + assemble the portable folder & zip under dist-portable/Quotio_<version>_x64_portable
npm run release
```

`major` = breaking/incompatible change, `minor` = new feature (backward compatible), `patch` = fixes/tweaks only. Distribute the built `.zip` / `.msi` / setup `.exe` via GitHub Releases — they are not committed to the repo.

## Migration rule

Do not copy SwiftUI/AppKit implementation details directly. Use the original app only as a reference for data contracts, state transitions, and user-facing behavior.