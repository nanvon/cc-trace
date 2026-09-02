<p align="center">
  <img src="design/brand/app-icons/app-icon-master-1024.png" width="128" alt="CC Trace Logo">
</p>

<h1 align="center">CC Trace</h1>

<p align="center">
  <b>Cross-Platform Desktop AI Quota Monitor & Local Usage Dashboard</b><br>
  Persistent macOS menu bar and Windows system tray utility for real-time Codex and Claude Code quotas, multi-engine token consumption, and cost trends.
</p>

<p align="center">
  <img alt="Platform" src="https://img.shields.io/badge/platform-macOS%20%7C%20Windows-000000?logo=apple&logoColor=white">
  <img alt="Tauri" src="https://img.shields.io/badge/Tauri-v2-24C8DB?logo=tauri&logoColor=white">
  <a href="https://github.com/nanvon/cc-trace/releases/latest"><img alt="Latest Release" src="https://img.shields.io/github/v/release/nanvon/cc-trace?color=brightgreen"></a>
  <img alt="Downloads" src="https://img.shields.io/github/downloads/nanvon/cc-trace/total?color=blue">
  <img alt="License" src="https://img.shields.io/badge/license-MIT-orange">
</p>

<p align="center">
  <a href="https://github.com/nanvon/cc-trace/releases/latest">Download</a> ·
  <a href="#-features">Features</a> ·
  <a href="#-screenshots">Screenshots</a> ·
  <a href="#-installation">Installation</a> ·
  <a href="#-data--security">Data & Security</a> ·
  <a href="#-building-from-source">Build from Source</a> ·
  <a href="#-related-projects">Related Projects</a> ·
  <a href="https://github.com/nanvon/cc-trace/issues">Issues</a> ·
  <a href="README.md">简体中文</a>
</p>

> [!WARNING]
> This project is in rapid iteration (`0.1.x` release). Core quota retrieval and session indexing are complete; features and UX details may be refined based on real-world feedback. Windows hardware validation is ongoing.

---

## ✨ Features

### ⚡ Cross-Platform Presence & Compact Quota
- **Dual-Platform Native Presence** — Runs in the macOS menu bar and Windows system tray, rendering real-time percentage indicators on macOS and tray tooltips with status indicators on Windows.
- **Frosted Compact Panel** — Instant popover window with native click-outside dismissal (macOS transient popover behavior), presenting 5-hour primary quota, 7-day weekly quota, and reset countdowns for Codex and Claude Code.
- **Service Health Monitoring** — Fetches live status chains from official OpenAI and Anthropic Statuspage feeds, rendering real-time service availability indicators in provider headers.
- **Fault Isolation & Smart Backoff** — Manages request lifecycles per provider independently, with 429 rate limit backoff and cross-provider fault isolation, preserving the last known valid snapshot during network anomalies.

### 📊 Local Usage & Cost Insights
- **Multi-Engine Session Aggregation** — Incrementally parses local session logs across four AI coding assistants: Codex, Claude Code, Pi, and OpenCode (JSONL and SQLite formats).
- **Multi-Dimensional Metrics** — Filters across today, yesterday, past 7 days, past 30 days, this week, this month, this year, or custom date ranges; details input, output, reasoning output, and cache hit rates alongside stacked daily usage charts.
- **Lifecycle Conversation Inspection** — Dedicated Conversations view supports search by title and project, sorting by multiple fields, displaying session start/end timestamps, elapsed duration, git branches, token breakdown, and API-equivalent cost estimations.
- **Quota Event Timeline** — Dedicated Timeline view plots step changes in quota snapshots across successful refreshes, isolating account sequences with irreversible fingerprints to visualize burn rates.

### 🛡️ Pure & Resilient Desktop Architecture
- **Zero-Config Discovery** — Automatically detects local terminal credentials without manual API key entry; gracefully handles missing credentials for one service without disrupting the other.
- **Lightweight & System-Integrated** — Built on Tauri v2 and Rust 2024 with a clean Vue 3 interface, featuring low memory usage, launch at login, dark/light theme switching, and system language detection.

---

### 📸 Screenshots

| Menu Bar / Tray Quota Panel | Local Usage Statistics & Conversations |
| :---: | :---: |
| _pending `docs/images/popover-light.png`_ | _pending `docs/images/statistics-overview.png`_ |

> [!NOTE]
> Screenshot and animation assets are being produced. Production release packages include the complete dark/light adaptive frosted compact panel, multi-dimensional charts, and conversation detail views.

---

## 📦 Installation

🍎 **System Requirements**:
- **macOS**: macOS 13.0 (Ventura) or later, with native support for Apple Silicon (M series) and Intel architectures;
- **Windows**: Windows 10 22H2 or later (64-bit), requires Microsoft Edge WebView2 runtime (pre-installed on Windows 11).

### 1. Download Prebuilt Binaries
Download the appropriate package from the [Releases page](https://github.com/nanvon/cc-trace/releases/latest):

| OS / Architecture | Release File | Notes |
| :--- | :--- | :--- |
| **macOS Apple Silicon** | `CC-Trace_<version>_macOS-Apple-Silicon.dmg`<br>`CC-Trace_<version>_macOS-Apple-Silicon.zip` | For M1/M2/M3/M4 Macs (portable `.zip` recommended) |
| **macOS Intel** | `CC-Trace_<version>_macOS-Intel.dmg`<br>`CC-Trace_<version>_macOS-Intel.zip` | For x86_64 Intel Macs |
| **Windows x64** | `CC-Trace_<version>_Windows-x64-Setup.exe` | 64-bit NSIS installer (downloads WebView2 if missing) |

### 2. System Clearance & Permissions
Because open-source releases are distributed without commercial Apple Developer ID notarization or Microsoft code-signing certificates, initial system blocks are standard OS security behavior:

* **macOS Gatekeeper Approval**: When blocked, navigate to **System Settings → Privacy & Security**, scroll down to the CC Trace notice, and click **"Open Anyway"**.
* **macOS Keychain Permission**: When reading Claude Code credentials for the first time, select **"Always Allow"** on the prompt (selecting "Allow" will re-trigger the prompt on subsequent launches).
* **Windows SmartScreen Approval**: When the blue SmartScreen prompt appears, click **"More info" → "Run anyway"**.

> [!NOTE]
> Starting with macOS Sequoia, the legacy "right-click → Open" bypass has been removed; you must use the System Settings route above.
> If macOS reports that the application is "damaged", remove the quarantine attribute via Terminal:
> ```bash
> xattr -dr com.apple.quarantine "/Applications/CC Trace.app"
> ```

Each release provides a `SHA256SUMS.txt` file for download verification:
```bash
# macOS
shasum -a 256 -c SHA256SUMS.txt --ignore-missing

# Windows (PowerShell)
Get-FileHash CC-Trace_*_Windows-x64-Setup.exe -Algorithm SHA256
```

---

## 🔒 Data & Security

CC Trace is a personal, open-source monitoring utility that operates entirely on your local machine. To aggregate quotas and parse session logs, it accesses system files under strict least-privilege principles:

### Credential & Permission Transparency
| Service / Module | Credential & Storage Path | Permissions | Mechanism & Security Guarantees |
| :--- | :--- | :---: | :--- |
| **Codex** | `~/.codex/auth.json` | Read / Write | Reads local OAuth credentials; atomically writes back refreshed `refresh_token` and provider metadata before expiry; never modifies other fields |
| **Claude Code** | `~/.claude/.credentials.json`<br>macOS Keychain | **Read / Write** | Reads file credentials with fallback to macOS Keychain; atomically writes back refreshed OAuth tokens; never relays credentials to third parties |
| **Session Logs** | `~/.codex/sessions`<br>`~/.claude/projects`<br>`~/.pi/agent/sessions`<br>`~/.local/share/opencode/` | **Strictly Read-Only** | Incrementally reads token counts and timestamps line-by-line; never modifies, truncates, moves, or deletes external log files |
| **Local Usage DB** | Application data directory `usage.db` (SQLite) | Read / Write | Derived metrics and quota history are stored in a local private database; accounts are isolated via irreversible fingerprints; plaintext credentials and prompt bodies never enter the database |

### Core Privacy Commitments
- **Zero Telemetry** — Contains no analytics, behavior tracking, or third-party tracking SDKs; external network requests are strictly limited to official OpenAI and Anthropic quota endpoints and public Statuspage feeds.
- **Kernel Isolation & Memory Safety** — Sensitive credentials are processed exclusively in the Rust core and encapsulated within `Secret` types with masked debug output, never exposed as plaintext tokens to the Vue frontend; the UI displays only redacted account names and numerical values.
- **Pure Text Parsing** — Does not capture raw network payloads or decrypt TLS streams; session bodies and absolute project paths never cross the IPC boundary into the frontend.
- **Transparent Cost Estimations** — API-equivalent costs are calculated strictly from public model rate cards for reference, and do not represent actual subscription billing.

> [!TIP]
> Prebuilt release binaries are compiled directly by official GitHub Actions workflows. If you prefer not to run ad-hoc signed executables, you can audit the code and [build from source](#-building-from-source).

---

## 🔧 Building from Source

### Prerequisites
- **Node.js**: `22.x` or later
- **pnpm**: `11.x` or later
- **Rust**: Stable toolchain (supporting 2024 Edition)
- **Platform Dependencies**: Follow [Tauri 2 Prerequisites](https://v2.tauri.app/start/prerequisites/) (Xcode Command Line Tools for macOS; Visual Studio C++ Build Tools for Windows).

### Local Development
```bash
# Clone repository
git clone https://github.com/nanvon/cc-trace.git
cd cc-trace

# Install dependencies
pnpm install

# Launch development environment (Vite frontend + Tauri desktop shell)
pnpm tauri dev
```

### Release Packaging
```bash
# macOS: Build .app, .dmg, and portable .zip
pnpm build:mac:release

# Windows: Build 64-bit NSIS installer (.exe)
pnpm tauri build
```
Artifacts are generated in the corresponding subdirectories under `src-tauri/target/release/bundle/`.

---

## 🔗 Related Projects

AI quota monitoring suite by the same author, sharing identical quota accounting rules and design language:

| Project | Type / Tech Stack | Target Platforms & Scenarios |
| :--- | :--- | :--- |
| [**cc-bar**](https://github.com/nanvon/cc-bar) | Native macOS menu bar app (SwiftUI) | Ultra-lightweight, native macOS status bar dashboard and desktop HUD |
| **CC Trace** (This repo) | Cross-platform desktop app (Tauri 2 + Rust + Vue 3) | macOS menu bar and Windows system tray with multi-engine session analytics and deep charting |
| [**cc-trace-mobile**](https://github.com/nanvon/cc-trace-mobile) | Companion mobile app (Flutter) | Monitor quota status and burn rates on the go across iOS and Android devices |

---

## 🙏 Acknowledgments

This project references and draws inspiration from the following open-source projects:

- [cc-switch](https://github.com/farion1231/cc-switch) — Multi-provider account switcher, inspiring credential discovery and multi-account handling.
- [cockpit-tools](https://github.com/jlcodes99/cockpit-tools) — Multi-platform AI coding assistant dashboard, providing key insights into quota refresh strategies and status visualization.
- [CodexBar](https://github.com/steipete/CodexBar) — macOS menu bar AI usage monitor, offering pioneer patterns in menu bar integration and local log parsing.
- [Tauri](https://github.com/tauri-apps/tauri) — Lightweight and fast application engine providing a solid foundation for cross-platform desktop shells.

---

## 📢 Disclaimer

This project is an independent open-source third-party utility and is not affiliated with, endorsed by, or sponsored by OpenAI or Anthropic. Codex, Claude, and related trademarks belong to their respective owners.

---

## 📄 License

This project is open-source under the [MIT License](LICENSE).
