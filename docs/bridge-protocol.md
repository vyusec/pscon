# pscon bridge protocol

The bridge is a loopback-only HTTP + WebSocket service started by:

```bash
pscon bridge serve
# or automatically by `pscon tui`
```

Default bind: `127.0.0.1:8787`.

## Authentication

- A 256-bit random token is generated on first start and stored at
  `$PSCON_HOME/bridge-token` with mode `0600`.
- The extension sends the token as a `token` query parameter for WebSocket
  upgrades and as `Authorization: Bearer <token>` for HTTP ingest.
- WebSocket upgrades also check the `Origin` header against
  `bridge.allowed_origins`.
- The server binds to loopback only and has a 64 MiB request-body limit.

## HTTP endpoints

| Method | Path | Auth | Purpose |
|---|---|---|---|
| GET | `/health` | no | Health check and protocol version |
| POST | `/ingest` | bearer | Upload a full `ScanSnapshot` |
| POST | `/ingest/resource` | bearer | Upload an authenticated resource body |
| GET | `/ws?token=...` | token | Control and live-event WebSocket |

## WebSocket messages

Client (extension) to pscon:

```json
{"type":"hello","protocol":1,"extension_version":"3.0.0","browser":"...","tab_url":"https://example.com"}
{"type":"scan_progress","scan_id":"...","stage":"collect","message":"...","progress":0.2}
{"type":"scan_event","event":{...ScanEvent...}}
{"type":"log","message":"..."}
{"type":"ping"}
```

pscon to client:

```json
{"type":"hello_ack","protocol":1,"session_id":"...","tool":"pscon","version":"0.1.0"}
{"type":"start_scan","options":{...}}
{"type":"cancel_scan"}
{"type":"fetch_resource","request_id":"...","url":"https://...","method":"GET","headers":{}}
{"type":"scan_progress","scan_id":"...","stage":"complete","message":"Complete: 12 endpoints, 0 secrets","progress":1.0}
{"type":"pong"}
```

Bulk snapshot data is sent over `POST /ingest` rather than the WebSocket to
avoid MV3 message-size limits and service-worker lifetime issues.

## Snapshot shape

See `pscon::core::ScanSnapshot`. The important fields are:

- `target` / `page_url`
- `html`
- `resources[]` (URL, kind, status, content type, inline content)
- `network[]` (URL, method, status, resource type, initiator)
- `inline_scripts[]`
- `storage_keys[]`
- `extension_version` and `captured_at`

## Pairing today

`pscon bridge serve` prints the token. Paste it into the JSner popup and click
**CONNECT**. A future version will add a user-approved pairing handshake so the
token never has to be copied manually.

## Scan execution and progress

Both `pscon tui` and `pscon bridge serve` start scans when a snapshot arrives.
One scan runs at a time; if another snapshot arrives, the newest snapshot is
queued. Scan stages, completion, and failures are forwarded to connected
extensions as `scan_progress` messages. JSner sends a heartbeat every 20 seconds
and restores its latest progress when the popup reopens.

For local browser regression checks, build the CLI with `cargo build --locked`,
install Python Playwright and its Chromium browser, then run
`python3 tests/connector_smoke.py`. The test uses the sibling `JSner` checkout
(or `JSNER_DIR`) and a temporary profile/database against a loopback fixture.
