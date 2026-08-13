<p align="center">
  <img src="design/brand/app-icons/app-icon-master-1024.png" width="128" alt="CC Trace icon">
</p>

<h1 align="center">CC Trace</h1>

<p align="center">A macOS menu bar / Windows tray utility: real-time remaining quota for Codex and Claude Code,<br>plus local token usage and cost statistics.</p>

<p align="center">
  <img alt="macOS" src="https://img.shields.io/badge/macOS-13%2B-000000?logo=apple&logoColor=white">
  <img alt="Windows" src="https://img.shields.io/badge/Windows-10%2B-0078D4">
  <img alt="Tauri" src="https://img.shields.io/badge/Tauri-24C8DB?logo=tauri&logoColor=white">
  <a href="https://github.com/nanvon/cc-trace/releases/latest"><img alt="release" src="https://img.shields.io/github/v/release/nanvon/cc-trace?color=brightgreen"></a>
  <img alt="license" src="https://img.shields.io/badge/license-MIT-orange">
</p>

<p align="center">
  <a href="https://github.com/nanvon/cc-trace/releases/latest">Download</a> ·
  <a href="#-installation">Install</a> ·
  <a href="#-building-from-source">Build from source</a> ·
  <a href="#-related-projects">Related projects</a> ·
  <a href="https://github.com/nanvon/cc-trace/issues">Feedback</a> ·
  <a href="README.md">简体中文</a>
</p>

<!-- Header screenshot placeholder: add light/dark quota panel shots under docs/images/ and replace this comment:
<p align="center">
  <img src="docs/images/popover-light.png" width="360" alt="Quota overview - light mode">
  <img src="docs/images/popover-dark.png" width="360" alt="Quota overview - dark mode">
</p>
-->

> [!WARNING]
> Still under active development; features and UX may keep changing. **The Windows side has not been verified on real hardware yet.**

## ✨ Features

- **Quota overview** — remaining quota, reset countdowns, and today / this-week cost for Codex and Claude Code, one click away on the menu bar / tray icon; accounts already logged in on this machine are detected automatically, no separate in-app login
- **Menu bar / tray display** — on macOS the remaining percentages of both services sit right next to the icon; on Windows they live in the tray tooltip; the context menu offers open, refresh, settings, and quit
- **Usage statistics** — token and cost totals for Codex and Claude Code, by today / yesterday / this week / this month / this year / last 7 days / last 30 days / all time / custom range, broken down by service and by model, with a per-day cost chart stacked by service; data comes from read-only scans of local session logs (JSONL), costs converted at public API prices
- **Preferences** — refresh interval, English/Chinese UI, light/dark theme, launch at login, pricing catalog updates
- **First-run onboarding** — explains the purpose and credential boundary, checks local Codex / Claude Code credentials; you can continue without any

### 📸 Screenshots

<!-- Screenshot placeholder: add the following assets under docs/images/ (light + dark) and replace this table.
     1. Side-by-side themes: popover-light.png / popover-dark.png, each <img width="360">
        (menu bar / tray quota panel: both services side by side, color-coded by remaining quota; reusable for the header slot)
     2. Large shot: statistics-overview.png <img width="720"> + a one-line <sub> caption
        (main window usage statistics: daily cost chart + per-model table)
-->

|              Quota overview               |               Usage statistics                |
| :---------------------------------------: | :-------------------------------------------: |
| _pending `docs/images/popover-light.png`_ | _pending `docs/images/statistics-overview.png`_ |

## 📦 Installation

🍎 Requires macOS 13 or later, or Windows 10 22H2 or later (64-bit). Codex / Claude Code must already be logged in on this machine (at least one) — with only one installed the other simply shows "no credentials found".

1. Download the file for your platform from [Releases](https://github.com/nanvon/cc-trace/releases/latest):

   | Platform            | File                                      |
   | ------------------- | ----------------------------------------- |
   | macOS Apple Silicon | `CC-Trace_<version>_macOS-Apple-Silicon.dmg` |
   | macOS Intel         | `CC-Trace_<version>_macOS-Intel.dmg`         |
   | Windows x64         | `CC-Trace_<version>_Windows-x64-Setup.exe`   |

   Not sure which Mac you have: Apple menu → "About This Mac", check the "Chip" line. Both macOS platforms also ship a same-named `.zip` with identical contents, no mounting needed.

2. macOS: open the DMG and drag `CC Trace.app` into `/Applications`. Windows: run the installer; it relies on the system WebView2 runtime, which Windows 11 and recent Windows 10 already include — if missing, the installer **downloads it from the network**.

3. CC Trace is not notarized by Apple and carries no Windows code-signing certificate, so being blocked once on first launch is expected: on macOS, after the blocked attempt, open **System Settings → Privacy & Security**, scroll down to the CC Trace prompt, and click **"Open Anyway"**; on Windows, choose **"More info → Run anyway"** in the SmartScreen prompt.

4. The first time macOS reads Claude Code's Keychain credentials, an authorization dialog appears — choose **"Always Allow"**; "Allow" only covers that single read and the dialog will return.

> [!NOTE]
> Since macOS Sequoia, the old "right-click → Open" workaround no longer works; the System Settings path above is the only way.
> If you still see "app is damaged", remove the quarantine attribute manually in Terminal:
>
> ```bash
> xattr -dr com.apple.quarantine "/Applications/CC Trace.app"
> ```

Every release ships a `SHA256SUMS.txt` for integrity verification:

```bash
# macOS
shasum -a 256 -c SHA256SUMS.txt --ignore-missing
```

## 🔒 Data & Security

CC Trace is a small open-source tool built for personal use. To query quotas, it reads local credentials:

- Codex: `~/.codex/auth.json`
- Claude Code: `~/.claude/.credentials.json` and the macOS Keychain
- Quota data is requested only from the official Codex and Claude Code endpoints; credentials are never sent to any third party — the only write to external data is renewing soon-to-expire credentials via the official OAuth flow
- Credentials are handled exclusively in the Rust core; the UI only receives redacted account info and quota numbers; tokens never enter logs or caches

Usage statistics are computed from read-only scans of the local Codex and Claude Code session logs (JSONL); the index is written only to CC Trace's own data directory.

> [!TIP]
> Released binaries are ad-hoc signed, not notarized by Apple, and not code-signed on Windows. If that concerns you, review the code yourself and [build from source](#-building-from-source) instead of relying on the released binaries.

## 🔧 Building from Source

Stack: Tauri 2 + Vue 3 + TypeScript + Rust. Requires Node.js 22+, pnpm 11, a stable Rust toolchain, and the [platform prerequisites required by Tauri](https://v2.tauri.app/start/prerequisites/).

**Daily development**: `pnpm install`, then `pnpm tauri dev`.

**Release packaging**:

```bash
pnpm build:mac:release   # macOS: outputs .app, .dmg, and .zip
pnpm tauri build         # Windows: outputs the NSIS installer
```

Artifacts land under the matching subdirectories of `src-tauri/target/release/bundle/`. Product and engineering docs start from the [documentation map](docs/README.md).

## 🔗 Related Projects

Three apps by the same author, sharing the same quota semantics and visual language:

|                                                                  |                                          |
| ---------------------------------------------------------------- | ---------------------------------------- |
| [**cc-bar**](https://github.com/nanvon/cc-bar)                   | Native macOS menu bar version (SwiftUI)  |
| **CC Trace** (this repository)                                   | Desktop · macOS menu bar / Windows tray  |
| [**CC Trace Mobile**](https://github.com/nanvon/cc-trace-mobile) | Mobile · iOS / Android                   |

CC Trace rebuilds cc-bar's feature set on Tauri to support both macOS and Windows. The three apps are independent; data and settings are not shared.

## 🙏 Acknowledgments

The design and implementation drew on these open-source projects (carried over from the predecessor [cc-bar](https://github.com/nanvon/cc-bar)):

- [cc-switch](https://github.com/farion1231/cc-switch) — multi-provider account switcher; inspired the multi-account management and import flow
- [cockpit-tools](https://github.com/jlcodes99/cockpit-tools) — multi-platform AI coding assistant dashboard; a reference for quota and refresh strategies
- [CodexBar](https://github.com/steipete/CodexBar) — macOS menu bar AI usage monitor; informed the menu bar interaction and local parsing approach

## 📢 Disclaimer

This project is not an official product of OpenAI or Anthropic, is not affiliated with either company, and is not endorsed or supported by them. Codex, Claude, and related names belong to their respective owners.

## 📄 License

[MIT](LICENSE)
