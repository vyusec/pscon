# pscon

`pscon` is a browser-assisted reconnaissance console for bug hunters and
penetration testers. It pairs with the JSner browser extension, turns the live
browser session into a structured asset inventory, then expands it with deep
JavaScript parsing, source-map recovery, crawling, passive secret detection,
safe endpoint verification, and typed decision-model triage.

The primary interface is a Ratatui TUI. A headless CLI is available for scripts
and CI.

## Features

- Local WebSocket bridge to JSner v3 (token-authenticated, loopback-only).
- Browser snapshot ingest: HTML, resources, network requests, inline scripts,
  storage keys.
- Same-origin crawler and well-known file collection.
- OXC-based JavaScript AST extraction, HTML/CSS/JSON/XML/GraphQL parsing.
- Source-map recovery, including `sourcesContent` parsing.
- URL normalization, deduplication, and path-template clustering.
- Passive secret detection with masked storage and reports.
- Safe endpoint verification (`GET`/`HEAD`/`OPTIONS` by default).
- Laya → Jev → GPT-6 Luna decision cascade with heuristics fallback.
- Interactive recon console with ASCII welcome screen, scan setup, recent scans,
  command palette, sortable findings, detail panels, and report export.
- JSON, Markdown, HTML, and SARIF reports.
- Desktop, ntfy, and webhook notifications.

## Install

Install or update the CLI from crates.io with Rust 1.98 or newer and a C/C++ build
toolchain:

```bash
cargo install pscon --locked
pscon --version
```

The package and installed command are both named `pscon`. Ensure
Cargo's bin directory (normally `~/.cargo/bin`) is on your `PATH`.
Cargo builds the binary from source. Install the browser extension from its own
[JSner repository](https://github.com/vyusec/JSner); see
[Extension setup](#extension-setup).

Maintainers: see [Publishing to crates.io](docs/publishing.md) for the first
release and automated publishing setup.

## Requirements

- Rust 1.98.0 (pinned by `rust-toolchain.toml`)
- Chrome/Chromium or Firefox for the extension
- Optional: Python 3.12 for local Laya decisions
- Optional: `JEV_API_KEY` and/or `OPENAI_API_KEY` for cloud escalation

## Build from source

```bash
git clone https://github.com/vyusec/pscon.git
cd pscon
cargo build --release
# binary: target/release/pscon
```

## Packaging

Build a self-contained release bundle:

```bash
./scripts/package.sh
# dist/pscon-<version>-<target>.tar.gz
```

The archive contains the `pscon` binary, docs, schemas, install scripts, and
`config.example.toml`. JSner is developed and packaged in its own repository.

## Quick start

```bash
# Create a config file
pscon config init

# Headless scan
pscon scan https://example.com

# TUI with the local bridge (also the default when no subcommand is given)
pscon tui
pscon

# Bridge only
pscon bridge serve
```

## Interactive console

Run `pscon` or `pscon tui` to open the welcome screen. Choose **New scan** to
enter an HTTP/HTTPS target and adjust crawl depth, page and asset limits,
verification, decision providers, and notifications. `pscon tui <url>` opens the
same form with the URL filled in. Settings apply to the current session.

Use Tab / Shift+Tab to move between form fields, Space to toggle an option,
and the **Start scan** button to submit. Ctrl+Enter also submits on terminals
that support that key combination. Text fields support cursor movement,
Home/End, Delete/Backspace, Ctrl+U to clear, Unicode, and bracketed paste.
Escape cancels the form without changing committed settings.

| Key | Action |
| --- | --- |
| `s` | New scan form |
| `h` / `H` | Welcome screen / recent scans |
| `1`–`6` | Overview, Endpoints, Secrets, Assets, Decisions, Report |
| Tab / Shift+Tab | Next / previous workspace view |
| Ctrl+P | Searchable command palette |
| Enter | Inspect a finding or open a saved scan |
| `/` | Live search in the current results; Enter applies, Escape restores |
| Escape | Close dialog or clear the current filter |
| `o` / `O` | Choose sort column / reverse sort direction |
| `j` / `k`, arrows | Move selection |
| `g` / `G`, Home / End | First / last result |
| Page Up / Page Down | Move ten results; scroll in details |
| `[` / `]` | Older / newer overview activity |
| `n` | Start a new scan using the selected history target |
| `r` | Reload current scan or refresh history |
| `e` | Regenerate JSON, Markdown, HTML, and SARIF reports |
| `?` | Help |
| `q` / Ctrl+C | Quit (Ctrl+C also works inside inputs) |

History shows the latest 50 scans. Search and sort settings are retained per
view, and finding selection stays attached to the same record as results arrive.
Secret details show masked values and omit source context. Report export runs
in the background and displays output paths or errors in the Report view.

The console accepts one active scan at a time. Saved results can be opened and
reports exported after it finishes. JSner snapshots start scans automatically;
a snapshot received during a scan is queued, with the newest queued snapshot
replacing any previous one. The layout adapts to narrow terminals; minimum size
is 60 columns by 18 rows.

## Extension setup

Clone the separate [JSner repository](https://github.com/vyusec/JSner), or
download and extract its source archive:

```bash
git clone https://github.com/vyusec/JSner.git
cd JSner
./scripts/install.sh
```

1. Open `chrome://extensions`.
2. Enable Developer mode.
3. Load the `JSner/` directory (containing `manifest.json`) as an unpacked extension.
4. Start the bridge with `pscon tui` (or `pscon bridge serve`) and get the
   token with `pscon bridge token`.
5. Open the JSner popup, paste the token, click **CONNECT**, then **DEEP SCAN
   WITH PSCON**.

The extension sends a snapshot to the bridge. If `pscon tui` is running, the TUI
starts a scan automatically.

## Commands

```text
pscon scan <url>              Headless scan
pscon tui [url]               Interactive TUI (starts the bridge)
pscon bridge serve            Local WebSocket/HTTP bridge
pscon bridge token            Print the extension bridge token
pscon ingest <file>           Import a pscon snapshot or raw JSner export
pscon report <scan-id>        Regenerate reports
pscon diff <a> <b>            Compare two scans
pscon scans                   List recent scans
pscon decisions test          Show providers and key availability
pscon decisions setup-laya    Create the Python 3.12 Laya venv
pscon decisions eval <file>   Evaluate endpoint-role decisions
pscon notify test             Send a test notification
pscon config init|show        Manage configuration
```

Useful scan flags:

```bash
pscon scan https://example.com \
  --depth 2 --max-pages 200 --max-assets 1000 \
  --no-decisions --no-verify --no-notify \
  --scope scope.txt \
  --header "Authorization: Bearer ..." \
  --cookie "session=..."
```

## Configuration

Default config path: `~/.config/pscon/config.toml`.
Data directory: `~/.local/share/pscon`.

Set `PSCON_HOME` to override the data directory and `PSCON_CONFIG` to override
the config file.

Key sections:

```toml
[recon]
crawl_depth = 2
max_pages = 200
max_assets = 1000
scan_secrets = true
verify = true
safe_methods = ["GET", "HEAD", "OPTIONS"]

[bridge]
bind = "127.0.0.1"
port = 8787

[decisions]
enabled = true
provider_order = ["laya", "jev", "luna"]
confidence_threshold = 0.75
max_cost_usd = 0.50

[decisions.laya]
base_url = "http://127.0.0.1:8766"
autostart = true
venv = "~/.local/share/pscon/laya-venv"

[notify]
desktop = true
ntfy_topic = ""
```

## Laya

Laya requires Python 3.10–3.13. Python 3.14 currently breaks `torch.compile` in
the published release. pscon defaults to `python3.12`.

```bash
pscon decisions setup-laya
# then run a scan; pscon autostarts laya-serve on 127.0.0.1:8766
```

## Safety

`pscon` is a reconnaissance tool. Only scan systems you own or have explicit
permission to test. Verification defaults to safe HTTP methods; state-changing
methods require `--unsafe-verify`. Use a scope file to enforce boundaries.

See `docs/security.md` for the threat model.

## Repository layout

```text
src/core/              config, models, SQLite, scope, notifications
src/engine/            HTTP, parsers, decisions, bridge, reports
src/tui/               ratatui interface
src/main.rs            pscon binary
docs/                  architecture, bridge protocol, decisions, security
tests/fixtures/        local recon fixture
```

## Working on both projects

Keep the two repositories in sibling folders:

```text
Documents/
├── pscon/     CLI, TUI, engine, bridge, and protocol docs
└── JSner/     browser extension and its own packaging scripts
```

Each folder has its own Git history and `origin` remote. Build and release each
project from its own folder; neither build needs the other checkout. The projects
communicate through the [localhost bridge protocol](docs/bridge-protocol.md).

## Licensing

The `pscon` Rust package is Apache-2.0; see [LICENSE](LICENSE). The separate
[JSner extension](https://github.com/vyusec/JSner) is MPL-2.0.
