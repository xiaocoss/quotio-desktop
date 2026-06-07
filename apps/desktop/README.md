# Quotio Desktop

Tauri-based cross-platform desktop shell for Quotio.

This app is the migration target for macOS, Windows, and Linux. The current SwiftUI macOS app remains in place while the cross-platform shell grows feature by feature.

## Scope in this stage

- React desktop UI shell
- Tauri command bridge
- Shared Rust workspace crates
- Platform information command
- Settings round-trip command
- Tray entry with open and quit actions

## Commands

```bash
npm install
npm run build
npm run tauri dev
npm run tauri build
```