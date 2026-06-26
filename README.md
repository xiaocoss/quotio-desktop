# Quotio

[![release](https://img.shields.io/github/v/release/xiaocoss/quotio-desktop?color=orange&label=release)](https://github.com/xiaocoss/quotio-desktop/releases/latest)
![platform](https://img.shields.io/badge/platform-macOS%20%7C%20Windows%20%7C%20Linux-blue)
[![downloads](https://img.shields.io/github/downloads/xiaocoss/quotio-desktop/total?color=success)](https://github.com/xiaocoss/quotio-desktop/releases)
[![stars](https://img.shields.io/github/stars/xiaocoss/quotio-desktop)](https://github.com/xiaocoss/quotio-desktop/stargazers)
[![license](https://img.shields.io/github/license/xiaocoss/quotio-desktop)](LICENSE)

> **Note**: This is the English version of Quotio Desktop. Full credit for the original Chinese version goes to [xiaocoss/quotio-desktop](https://github.com/xiaocoss/quotio-desktop).

**A cross-platform AI account quota management tool** —— Uses a local [CLIProxyAPI](https://github.com/router-for-me/CLIProxyAPI) proxy to unify and manage the quotas and invocations of multiple AI accounts. It supports multi-account rotation, quota monitoring, smart scheduling, and multiple Codex instances.

Currently supports providers such as **Codex (OpenAI)**, **Claude Code**, **GitHub Copilot**, **Gemini CLI**, **Antigravity**, **Kiro**, **Cursor**, **Trae**, **GLM**, and more.

> This project is a cross-platform port of [quotio](https://github.com/nguyenphutrong/quotio) (originally macOS / SwiftUI), rewritten in Tauri, using one codebase for macOS / Windows / Linux.

📥 **[Download the latest version](https://github.com/xiaocoss/quotio-desktop/releases/latest)** —— macOS / Windows / Linux (or see [Installation Guide](#-installation) below)

---

## ✨ Features

- **Multi-Account Management** —— Manage multiple AI accounts in a unified proxy pool with automatic rotation and on-demand enabling/disabling.
- **Quota Monitoring** —— View plans, expiration dates, Session (5-hour), and Weekly window usage per account. Codex accounts additionally show "Active Reset Counts" and support **one-click resets** for the 5h window.
- **Smart Scheduling** —— "Closest to refresh priority": Only allows the Codex account closest to its 5h window refresh into the pool while keeping others on standby. Switches automatically to avoid wasting quota.
- **Usage Analytics** —— Dashboard displaying invocation counts, success rates, Token usage, and estimated costs, filterable by account / provider / model.
- **CLI Integration** —— Point CLI tools like Claude Code, Codex, and Gemini CLI to the local CLIProxyAPI with one click.
- **Codex Multi-Instancing** —— Bind multiple instances to different accounts and run them in parallel without interference.
- **And More** —— Request logging, fallback strategies (experimental), and configuration backup/deduplication.

## 🖥️ Supported Platforms

**macOS** · **Windows** · **Linux**

## 🌐 Interface Languages

English · 简体中文

## 📸 Screenshots

<p align="center">
  <img src="assets/dashboard.png" width="820" alt="Quotio Dashboard"><br>
  <sub><b>Dashboard</b> —— Invocation counts, success rates, token usage, and estimated costs, filterable by account / provider / model</sub>
</p>

<p align="center">
  <img src="assets/quota.png" width="820" alt="Quotio Quota"><br>
  <sub><b>Quota</b> —— Session (5h) / Weekly windows, plan tiers, expirations, Codex active resets + one-click reset</sub>
</p>

<details>
<summary>More screens (Providers / Agents / Logs / Settings)</summary>

<p align="center">
  <img src="assets/providers.png" width="820" alt="Providers"><br>
  <sub><b>Providers</b> —— Multi-account OAuth proxy pool management</sub>
</p>
<p align="center">
  <img src="assets/agents.png" width="820" alt="Agents"><br>
  <sub><b>Agents</b> —— Point CLI tools to CLIProxyAPI</sub>
</p>
<p align="center">
  <img src="assets/logs.png" width="820" alt="Logs"><br>
  <sub><b>Logs</b> —— Per-request history: Tokens, elapsed time, status</sub>
</p>
<p align="center">
  <img src="assets/settings.png" width="820" alt="Settings"><br>
  <sub><b>Settings</b> —— App modes, proxy, language, themes, privacy</sub>
</p>

</details>

## 📦 Installation

### Option A: Download Installer (Recommended)

Click the links below to download **v0.5.0**, or head to [**Releases**](https://github.com/xiaocoss/quotio-desktop/releases/latest) for the latest version:

| Platform | Download |
|---|---|
| **Windows** | [`.msi` (Recommended)](https://github.com/xiaocoss/quotio-desktop/releases/download/v0.5.0/Quotio_0.5.0_x64_en-US.msi) · [`-setup.exe`](https://github.com/xiaocoss/quotio-desktop/releases/download/v0.5.0/Quotio_0.5.0_x64-setup.exe) |
| **macOS** (Apple Silicon & Intel Universal) | [`.dmg`](https://github.com/xiaocoss/quotio-desktop/releases/download/v0.5.0/Quotio_0.5.0_universal.dmg) |
| **Linux** | [`.deb`](https://github.com/xiaocoss/quotio-desktop/releases/download/v0.5.0/Quotio_0.5.0_amd64.deb) · [`.rpm`](https://github.com/xiaocoss/quotio-desktop/releases/download/v0.5.0/Quotio-0.5.0-1.x86_64.rpm) · [`.AppImage`](https://github.com/xiaocoss/quotio-desktop/releases/download/v0.5.0/Quotio_0.5.0_amd64.AppImage) |

> Note: macOS packages are currently unsigned. If you see "App is damaged / cannot be opened", go to "System Settings → Privacy & Security" and click "Open Anyway".

### Option B: Build from Source

Ensure you have [Node.js](https://nodejs.org) (18+) and [Rust](https://rustup.rs) installed.

```bash
git clone https://github.com/xiaocoss/quotio-desktop.git
cd quotio-desktop
npm run desktop:install
npm run desktop:build      # Output: target/release/ and target/release/bundle/
```

## 🔧 Development

```text
apps/desktop/         Tauri + React Desktop App
crates/
├── quotio-types/     Shared Data Models (Provider / Account / Quota / Log / Settings)
├── quotio-core/      Cross-platform Proxy, State, and Management API Domain Layer
└── quotio-platform/  OS Adaptation for System Tray / Auto-start / Notifications / Paths
```

```bash
npm run desktop:dev            # Development mode (Tauri)
npm run cargo:check            # Rust linter check
npm run version:set -- minor   # Bump version (major | minor | patch | X.Y.Z)
npm run release                # Compile + assemble portable builds (dist-portable/)
```

**Release Flow**: `npm run version:set -- <bump>` → commit → `git tag vX.Y.Z && git push origin vX.Y.Z`. Pushing a `v*` tag triggers GitHub Actions to automatically build and publish to Releases for Windows / macOS / Linux.

## 🙏 Acknowledgements

- **[xiaocoss/quotio-desktop](https://github.com/xiaocoss/quotio-desktop)** —— The original Chinese project this English version is based on.
- **[CLIProxyAPI](https://github.com/router-for-me/CLIProxyAPI)** —— The core local proxy service that this project relies on.
- **[kiro.rs](https://github.com/hank9999/kiro.rs)** —— Kiro (AWS CodeWhisperer) → Anthropic compatible proxy, built-in as a sidecar for the Kiro proxy pool.
- **[cockpit-tools](https://github.com/jlcodes99/cockpit-tools)** —— Similar project, used as a reference for product and documentation.
- **[quotio](https://github.com/nguyenphutrong/quotio)** —— Inspiration for the interface and original macOS version.

Thanks to the authors of the above open-source projects! If they helped you too, please give them a ⭐ Star.

## 📄 License

[MIT](LICENSE) © 2025 Trong Nguyen
