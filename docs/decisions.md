# Decision models

pscon uses typed decision models, not generative chat models, for recon triage.
The model receives a state (endpoint, secret candidate, or asset metadata) and a
set of typed questions, and returns calibrated `choice`, `score`, and `yes/no`
answers.

## Providers

| Provider | Endpoint | Notes |
|---|---|---|
| Laya | `POST /v1/systemone` on a local `laya-serve` | Free, fast, Jev-compatible |
| Jev | `POST https://jevtypesafeai.com/api/v1/decide` | `JEV_API_KEY` |
| GPT-6 Luna | `POST https://api.openai.com/v1/decisions` | `OPENAI_API_KEY`, model `gpt-6-luna` |

## Cascade

Default order:

1. Deterministic heuristics (always).
2. Laya local (if reachable).
3. Jev (if key present and Laya is low-confidence).
4. GPT-6 Luna (if key present and still low-confidence).

A provider is skipped when its key is missing or its health check fails. The
cascade stops early when every answer clears `decisions.confidence_threshold`.
Per-scan budgets are enforced by `max_calls` and `max_cost_usd`.

## Question packs

### Endpoint triage

- `role`: api, page, static, auth, admin, upload, graphql, websocket, config,
  docs, telemetry, noise.
- `sensitivity`: 0–4 ordinal score.
- `auth_required`: calibrated yes/no.
- `verify_safe`: calibrated yes/no.
- `priority`: 0–4 ordinal score.

### Secret triage

- `is_real`: yes/no.
- `secret_class`: cloud, source_control, payment, communication, ai, database,
  private_key, jwt, generic, test_placeholder.
- `risk`: 0–4 ordinal score.
- `should_validate`: yes/no.

Cloud providers never receive raw secret values by default. The state includes
the masked value, rule, provider, severity, entropy, and surrounding context.

## Caching and budgets

- Decisions are cached in SQLite by a SHA-256 hash of the state and question set.
- `decisions.max_calls` caps model calls per scan.
- `decisions.max_cost_usd` caps cloud spend per scan.
- `decisions.redact_cloud` is enabled by default.

## Laya setup

Laya currently requires Python 3.10–3.13. Python 3.14 breaks `torch.compile` in
the published release.

```bash
pscon decisions setup-laya
# or
./scripts/setup-laya.sh
```

The default venv is `$PSCON_HOME/laya-venv` (usually
`~/.local/share/pscon/laya-venv`). pscon autostarts `laya-serve` on
`127.0.0.1:8766` when `decisions.laya.autostart = true`.

## Evaluation

```bash
pscon decisions eval labels.jsonl
```

Each line:

```json
{"state":{"endpoint":{"url":"/api/v1/admin/users","method":"GET"}},"gold":{"role":"admin"}}
```

The command reports accuracy, mean confidence, and cost for the endpoint-role
question. Use it to compare providers and to build a labelled set for Laya
fine-tuning.
