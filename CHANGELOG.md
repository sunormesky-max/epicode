# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [1.1.0](https://github.com/sunormesky-max/epicode/compare/v1.0.3...v1.1.0) (2026-09-26)


### Features

* frontend整体替换为生产级前端树(双树合一) ([#92](https://github.com/sunormesky-max/epicode/issues/92)) ([84910a8](https://github.com/sunormesky-max/epicode/commit/84910a870e5f0950b859325c858eb0c7070eeaaf))


### Bug Fixes

* CodeQL trivial-conditional ([#94](https://github.com/sunormesky-max/epicode/issues/94)) ([e0df34a](https://github.com/sunormesky-max/epicode/commit/e0df34aad47f74b2ace32d51ad9d0f42d22f8414))
* verify-version.sh在set -e下grep无匹配静默杀脚本 ([#88](https://github.com/sunormesky-max/epicode/issues/88)) ([366fa78](https://github.com/sunormesky-max/epicode/commit/366fa78f9cce110ee546f0b02269711b6e1b570f))
* 三轮审计全量修复 (1严重+4高+12中+垃圾清理) ([#90](https://github.com/sunormesky-max/epicode/issues/90)) ([3e1e224](https://github.com/sunormesky-max/epicode/commit/3e1e22419da401b4ed4d2d6505669374ca380e5d))
* 四轮审计残项全清(含两处上轮replace静默失败) ([#91](https://github.com/sunormesky-max/epicode/issues/91)) ([e32d3af](https://github.com/sunormesky-max/epicode/commit/e32d3afc539e1749d7ee23c38faf0787fdd45599))
* 审查发现A — nowSnapshot改数据加载时刷新(时间筛选不再用挂载时刻) ([#97](https://github.com/sunormesky-max/epicode/issues/97)) ([0e62f00](https://github.com/sunormesky-max/epicode/commit/0e62f00b655c3ace92cec4852a35286eb12d1037))

## [1.0.3](https://github.com/sunormesky-max/epicode/compare/v1.0.2...v1.0.3) (2026-09-25)


### Bug Fixes

* release-please extra-files的toml/yaml更新器改generic ([#84](https://github.com/sunormesky-max/epicode/issues/84)) ([232964a](https://github.com/sunormesky-max/epicode/commit/232964a1dd3f94e2a6f0fb533b8d571a38cb88ee))
* 审计二轮全量收尾 ([#86](https://github.com/sunormesky-max/epicode/issues/86)) ([d18ee88](https://github.com/sunormesky-max/epicode/commit/d18ee88c42378e5b436f6e1a9d6952f211ce9881))

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
