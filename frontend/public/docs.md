# Epicode API Reference (static mirror)

> Mirror of https://epicode.cn/#/docs for fetch-only visitors. Generated 2026-08-20 from the same source. Human page may add newer endpoints; llms.txt lists the canonical summary.

Auth: all /v1/* endpoints require `X-API-Key: tm-...` header (obtained at /register). Rate limits per plan: Free 60 req/min, Pro 300, Enterprise 1000.

## Auth
- POST /register {user_id, password, plan?} — create account; invite code (X-Invite-Code header) or admin gated. Returns {api_key: "tm-...", plan, max_memories}.
- POST /v1/login {user_id, password} — sets HttpOnly session cookie (epicode_session). Console uses cookie; API uses key.
- POST /v1/logout — clears session.

## Memory
- POST /v1/remember {content, labels?: string[]} — store a memory (auto embedding + classification + spatial placement). ~0.4ms write.
- POST /v1/search {query, mode?: exact|semantic|graph|hybrid|auto, limit?} — semantic search across modes.
- POST /v1/recall {query} — deep recall (semantic + knowledge-graph relations).
- GET /v1/memory/:id — fetch one memory.
- GET /v1/memory/list — list memories.
- PUT /v1/memory/:id — update.
- DELETE /v1/memory/:id — delete.
- GET /v1/timeline — memory timeline.
- SSE GET /v1/stream?key=... — live cognitive stream (energy, emotion, cognitive_status, drive signals). Short-lived tickets via /v1/stream/ticket for cookie sessions.

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
