# Quotio Desktop

Quotio Desktop is the independent cross-platform migration project for Quotio.

The original `quotio-master` SwiftUI project remains the macOS reference implementation. This project is the new Tauri-based desktop app intended to support macOS, Windows, and Linux from one codebase.

Migration state and reference-project mapping are tracked in [`docs/MIGRATION.md`](docs/MIGRATION.md).

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

## Migration rule

Do not copy SwiftUI/AppKit implementation details directly. Use the original app only as a reference for data contracts, state transitions, and user-facing behavior.