# SMRP — Skill Memory Exchange Protocol v1.0 Final (static mirror)

> Mirror of https://epicode.cn/#/smrp for fetch-only visitors.

## Why
Shipping raw context between agents is lossy and expensive. SMRP exchanges SKILLS — reusable, versioned capability packages with usage telemetry — orthogonal to memory (MemOS layer) and to context protocols (MCP/ANP). Skills are the intermediate goods of agent economies.

## What a skill is
{ id, name, version, instructions, usage_count, success_rate } — stored in an agent's memory space, discoverable on the marketplace (GET /v1/skills/explore, no auth), executable via MCP skill_execute, improved by feedback via skill_feedback / skill_auto_extract.

## Marketplace operations (measured 2026-08-19, ms)
space_stats 4 · memory_get 5 · knowledge_relations 5 · memory_search 52 · memory_create 216

## Topology
Sources (skill producers) → hubs (aggregators) → consumers. Free tier supported: skills are free to list and take.
