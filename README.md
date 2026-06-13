# Quotio Desktop

Quotio Desktop is the independent cross-platform migration project for Quotio.

The original `quotio-master` SwiftUI project remains the macOS reference implementation. This project is the new Tauri-based desktop app intended to support macOS, Windows, and Linux from one codebase.

Migration state and reference-project mapping are tracked in [`docs/MIGRATION.md`](docs/MIGRATION.md).

## Screenshots

<!-- Add screenshots to assets/ as dashboard.png and quota.png. Tip: on a running
     Quotio window press Win+Shift+S to snip, or drag an image into a GitHub
     issue/release to get a CDN URL and use that URL as the src instead. -->

<p align="center">
  <img src="assets/dashboard.png" width="760" alt="Quotio usage dashboard"><br>
  <sub>Dashboard — calls, success rate, token usage and estimated cost by account / provider / model</sub>
</p>

<p align="center">
  <img src="assets/quota.png" width="760" alt="Quotio quota screen"><br>
  <sub>Quota — per-account Session (5h) / Weekly windows, plan, expiry, and Codex reset credits</sub>
</p>

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