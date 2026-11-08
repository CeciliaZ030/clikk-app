# Clikk
> Community Aomi app maintained by CeciliaZ030. Not an official Aomi Labs app.

Aomi app for the [Clikk Profile API](https://clikk.pro): fetch one member's
published public Clikk Card by exact handle.

## Tool

| Tool | Kind | Purpose |
|---|---|---|
| `get_clikk_profile` | read | Published card for a handle: display name, photo, contact fields, page + vCard links, consent flags. Optional `kinds` filter. `found: false` on 404. |

There is deliberately no search or listing tool: the upstream API only supports
exact-handle lookup, and 404 does not distinguish unpublished from nonexistent.

## Secrets

| Name | Required | Notes |
|---|---|---|
| `CLIKK_API_KEY` | yes | Clikk-issued `ck_...` key, sent as `Authorization: Bearer`. Operator-managed (Aomi's partner key). |
| `CLIKK_API_BASE_URL` | no | Defaults to the current Vercel deployment. Set to `https://clikk.pro` once Clikk PR #215 merges; paths and key are unchanged. |

## Build and test

```bash
cargo build
cargo test
# live check against the real API
CLIKK_API_KEY=ck_... cargo test -- --ignored
```

`test.json` seeds the e2e harness (`aomi-app-e2e-tester`).
