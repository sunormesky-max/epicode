# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- MCP Registry discovery JSON with 35 standardized tools (`.well-known/mcp/discovery.json`).
- Python SDK with SMRP tiered recall, identity rituals, and knowledge graph support.
- TypeScript SDK with differentiated features beyond basic remember/search.
- End-to-end AI Agent memory example (`examples/python/ai_agent_memory.py`).
- Release strategy documentation (`RELEASE_STRATEGY.md`).
- GitHub community health files: issue templates, PR template, CODE_OF_CONDUCT.md, SECURITY.md.
- GitHub Actions workflows: greetings.yml, discussions.yml, scorecard.yml.
- CI optimization with `paths-ignore` for markdown and docs changes.

### Changed

- Environment variable prefix migrated from `TETRAMEM_` to `EPICODE_` across 26 files.

### Fixed

- Backend compilation warnings in `backend/src/engine/mod.rs`.


## [1.0.1] - 2026-06-21

### Fixed

- Fixed admin authentication bypass in `cloud.rs` by rejecting empty `admin_key` and missing/empty `X-Admin-Key` header.
- Converted `blocking()` wrapper to return `Result<T, String>` instead of panicking when a `spawn_blocking` task fails.
- Added `SecurityConfig::try_from_env()` to avoid startup panic; cloud mode now exits gracefully on missing `EPICODE_API_KEY`.

### Security

- Replaced `ureq` with `attohttpc` in cognitive, embedding, and classifier modules.
- Limited Cloud TCP server concurrency with a `tokio::sync::Semaphore` to prevent unbounded OS thread creation.

### Changed

- Upgraded `ort` from `2.0.0-rc.9` to `2.0.0-rc.12`.

## [1.0.0] - 2026-06-20

### Added

- Initial open-source release of Epicode.
- Spatial AI memory system: tetrahedron storage, HNSW + BM25 search, knowledge graph.
- MCP integration with 35 standardized tools and SMRP response protocol.
- Multi-tenant Cloud mode with user management, quotas, and invite codes.
- React 19 frontend dashboard and Rust Axum backend.
- `epicode-guard` defense system for SSH/Web/honeypot protection.
- Docker Compose and Kubernetes deployment templates.
- Repository docs, issue/PR templates, Dependabot configuration, and MIT license.

[Unreleased]: https://github.com/sunormesky-max/epicode/compare/v1.0.1...HEAD
[1.0.1]: https://github.com/sunormesky-max/epicode/compare/v1.0.0...v1.0.1
[1.0.0]: https://github.com/sunormesky-max/epicode/releases/tag/v1.0.0
