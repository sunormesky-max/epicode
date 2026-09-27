# Epicode API Reference (static mirror)

> Mirror of https://epicode.cn/#/docs for fetch-only visitors. Generated 2026-08-20 from the same source. Human page may add newer endpoints; llms.txt lists the canonical summary.

Public API base URL: `https://epicode.cn/api`. Authenticated endpoints accept an `X-API-Key` header or the HttpOnly `epicode_session` cookie; if both are sent, they must belong to the same account. JSON request bodies use `Content-Type: application/json`. The console uses the cookie; external API clients can use the key returned once at registration. Rate limits per plan: Free 60 req/min, Pro 300, Enterprise 1000.

## Auth
- POST /register {user_id, password, plan?} — create account; invite code (X-Invite-Code header) or admin gated. Returns {success, user_id, api_key, plan, max_memories}.
- POST /v1/login {user_id, password} — sets the HttpOnly, Secure, SameSite=Strict `epicode_session` cookie. Returns {success, user_id, plan, max_memories}; the JSON body does not contain an API key.
- POST /v1/logout — clears the session cookie.

## Memory
- POST /v1/remember {content, labels?: string[]} — store a memory (auto embedding + classification + spatial placement). ~0.4ms write.
- POST /v1/search {query, mode?: exact|semantic|graph|hybrid|auto, limit?} — semantic search across modes.
- POST /v1/recall {query} — deep recall (semantic + knowledge-graph relations).
- GET /v1/memory/:id — fetch one memory.
- GET /v1/memory/list — list memories.
- PUT /v1/memory/:id — update.
- DELETE /v1/memory/:id — delete.
- GET /v1/timeline — memory timeline.
- POST /v1/stream/ticket — authenticated request that returns a short-lived, single-use stream ticket (`expires_in: 120`).
- SSE GET /v1/stream?ticket=YOUR_ONE_USE_TICKET — live cognitive stream (energy, emotion, cognitive_status, drive signals). Request a fresh ticket for every reconnect. Never put a long-lived API key in a URL.

## Identity (required before memory tools on a fresh account)
- POST /v1/identity/confirm — confirm agent identity (REST path of the MCP ritual).

## MCP (for agents)
- POST /mcp — 41 tools (memory_*, ctx_*, pattern_*, identity_*, skill_*, drive_*). Identity ritual required; see llms.txt for the exact 7-step sequence.

## Skills / SMRP
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
