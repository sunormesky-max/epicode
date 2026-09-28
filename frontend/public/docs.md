# Epicode API Reference (static mirror)

> Mirror of https://epicode.cn/#/docs for fetch-only visitors. Generated 2026-08-20 from the same source. Human page may add newer endpoints; llms.txt lists the canonical summary.

Public API base URL: `https://epicode.cn/api`. Authenticated endpoints accept an `X-API-Key` header or the HttpOnly `epicode_session` cookie; if both are sent, they must belong to the same account. JSON request bodies use `Content-Type: application/json`. The console uses the cookie; external API clients can use the key returned once at registration. Rate limits per plan: Free 60 req/min, Pro 300, Enterprise 1000.

## Auth
- POST /register {user_id, password, plan?} — create account; invite code (X-Invite-Code header) or admin gated. Returns {success, user_id, api_key, plan, max_memories}.
- POST /v1/login {user_id, password} — sets the HttpOnly, Secure, SameSite=Strict `epicode_session` cookie. Returns {success, user_id, plan, max_memories}; the JSON body does not contain an API key.
- POST /v1/logout — clears the session cookie.

## Memory
- POST /v1/remember {content, labels?: string[]} — store a memory (auto embedding + classification + spatial placement). ~0.4ms write.
- POST /v1/search {query, mode?: exact|semantic|graph|hybrid|auto|fusion, limit?} — search; `graph` expands hybrid-search seeds with KG-PPR, `auto` routes temporal/aggregation queries to graph+PPR and other queries to semantic, while `fusion` combines semantic and graph+PPR rankings.
- POST /v1/recall {query} — deep recall (semantic + knowledge-graph relations).
- GET /v1/memory/:id — fetch one memory.
- GET /v1/memory/list — list memories.
- PUT /v1/memory/:id — update.
- DELETE /v1/memory/:id — delete.
- GET /v1/timeline — memory timeline.
- POST /v1/stream/ticket — authenticated request that returns a short-lived, single-use stream ticket (`expires_in: 120`).
- SSE GET /v1/stream?ticket=YOUR_ONE_USE_TICKET — live cognitive stream (energy, emotion, cognitive_status, drive signals). Request a fresh ticket for every reconnect. Never put a long-lived API key in a URL.

## L0 Drive
- GET /v1/drive/inbox — SMRP envelope; `data` contains unacknowledged `signals`, `stats`, and `empty_reason`. Inbox includes pending and delivered signals and a `retryable` flag.
- POST /v1/drive/ack {drive_id, executed, outcome, reflection?} — acknowledge and provide execution feedback. Requires an active primary executor registered with POST /v1/runtime/register; POST /v1/runtime/heartbeat revives an expired binding.
- GET /v1/runtime/status — inspect the executor binding and lease.
- POST /v1/runtime/unregister — remove the current account's executor binding.
- Runtime register/status/heartbeat/unregister return an SMRP envelope when the account's engine is loaded and raw JSON otherwise; heartbeat reports `success: false` when no binding exists.
- MCP `drive_inbox` and `drive_ack` use the same argument and signal fields. `drive_inbox` defaults to a limit of 50.
- With an executor E2E public key registered, REST/MCP inbox and SSE encrypt descriptions as `description_e2e` and set `description` to null. High/Critical acknowledgements also require E2E enabled on the binding.
- SSE drive events are a projection containing newly enqueued signals. Browser EventSource clients must first mint a one-use ticket; see the stream endpoints above.

## Identity (required before memory tools on a fresh account)
- POST /v1/identity/confirm — confirm agent identity (REST path of the MCP ritual).

## MCP (for agents)
- POST /mcp — discover the current tools using MCP `tools/list`. Identity ritual required; see llms.txt for the setup flow.

## SMRP and skills
- GET /v1/smrp — canonical SMRP 1.0 specification for structured memory responses. SMRP is independent of MCP/REST transport; skills are a separate API/tool family.
- GET /v1/skills/explore — public skill marketplace listing.
- Skill tools via MCP: skill_execute, skill_feedback, skills_sync, skill_auto_extract.

## Stats
- GET /v1/stats — space statistics (memories, clusters, energy, tetrahedra, api calls).
- GET /stats/public — public aggregate stats, no auth.
- GET /health — liveness, no auth.

## Sub-accounts
- Main accounts manage sub-accounts via console (#/dashboard/accounts) and MCP admin tools.

## Example
curl -X POST https://epicode.cn/api/v1/remember \
  -H "Content-Type: application/json" \
  -H "X-API-Key: tm-your-key" \
  -d '{"content": "User prefers dark mode", "labels": ["preference"]}'
