# Architecture

```text
JSner v3 extension
  ├─ content script: network hooks + snapshot collector
  ├─ service worker: WebSocket control + HTTP ingest
  └─ popup: bridge pairing, deep scan
            │
            │ 127.0.0.1:8787 (token + origin check)
            ▼
pscon bridge (axum)
  ├─ POST /ingest          full browser snapshot
  ├─ POST /ingest/resource authenticated resource body
  └─ GET  /ws              control + live events
            │
            ▼
pscon engine
  ├─ collectors: browser snapshot, same-origin crawler, well-known files
  ├─ parsers: OXC JavaScript AST, HTML, CSS, JSON, XML, source maps
  ├─ normalizer: URL resolution, path templates, dedupe
  ├─ secrets: regex rules + entropy + context
  ├─ verification: safe methods, scope guard, rate limits
  ├─ decisions: Laya → Jev → GPT-6 Luna cascade
  └─ reports: JSON, Markdown, HTML, SARIF
            │
            ├─ SQLite: scans, assets, endpoints, secrets, verifications, decisions
            ├─ ratatui TUI
            └─ notifications: desktop + ntfy + webhooks
```

## Modules

The CLI and its internal modules are distributed as one `pscon` Cargo package.

| Module | Role |
|---|---|
| `core` | Config, domain models, SQLite storage, scope, events, notifications |
| `engine` | HTTP, parsers, normalization, secrets, verification, decisions, bridge, reports |
| `tui` | Ratatui interface and live event loop |
| `main.rs` | `pscon` binary and subcommands |

## Scan pipeline

1. Create a scan record and emit `started`.
2. Fetch the target. HTML is parsed; non-HTML is treated as an asset.
3. Collect resources from HTML, inline scripts, performance data, and the
   browser snapshot.
4. Crawl same-origin pages up to `crawl_depth` / `max_pages`.
5. Fetch assets up to `max_assets` with a per-host rate limit.
6. Parse JavaScript with OXC, source maps with the `sourcemap` crate, and
   HTML/CSS/JSON/XML with dedicated extractors.
7. Normalize and deduplicate endpoints, cluster path templates, and attach
   evidence.
8. Detect secret candidates with rules and entropy.
9. Run decision-model triage on the top candidates.
10. Optionally verify safe endpoints.
11. Persist, generate reports, and notify.
