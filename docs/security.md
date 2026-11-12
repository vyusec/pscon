# Security and safety model

pscon is a reconnaissance tool. Use it only on systems you own or have explicit
permission to test.

## Scope enforcement

- `scope.txt` supports exact hosts, `*.example.com` wildcards, CIDR ranges, and
  `!` deny rules.
- Every HTTP request and every redirect hop is checked against scope.
- The default scope is the target host plus its subdomains.

Example:

```text
example.com
*.example.com
!internal.example.com
10.0.0.0/24
```

## Verification defaults

- Safe methods only: `GET`, `HEAD`, `OPTIONS`.
- `POST`, `PUT`, `PATCH`, and `DELETE` require `--unsafe-verify`.
- Verification is rate-limited per host and respects redirect scope.
- pscon does not send exploit payloads; it records status, headers, body hash,
  and a redacted snippet.

## Secrets

- Secret values are masked in storage, reports, and cloud decision requests.
- Raw secret values are not stored by default.
- Validation is opt-in and intended for safe, non-destructive provider checks.

## Local bridge

- Binds to `127.0.0.1` only.
- 256-bit random token, stored with mode `0600`.
- WebSocket origin allowlist.
- No wildcard CORS on the authenticated ingest endpoints.
- 64 MiB body cap.

## Extension permissions

The v3 extension requests:

- `scripting`, `activeTab`, `tabs`, `storage` for the existing scanner.
- `webRequest` for initial-load network capture.
- `alarms` for bridge reconnect keepalive.
- Host permissions for `<all_urls>` and localhost bridge endpoints.

The extension does not send data anywhere except the local pscon bridge. Cloud
decision calls are made by the CLI, not by the browser extension.

## Data handling

- Scan data is stored locally in `$PSCON_HOME/pscon.db`.
- Reports are written under `$PSCON_HOME/scans/<scan-id>/`.
- Cloud providers receive only the structured state needed for a decision.
- Use `pscon decisions preview` (planned) or inspect `docs/decisions.md` to see
  what is sent.
