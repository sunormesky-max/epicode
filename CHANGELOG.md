# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [2.0.0](https://github.com/sunormesky-max/epicode/compare/v1.0.1...v2.0.0) (2026-06-27)


### ⚠ BREAKING CHANGES

* **sdk:** SDK package names changed from tetramem-sdk to epicode-sdk

### Features

* add example community skills ([b840178](https://github.com/sunormesky-max/epicode/commit/b840178232b7b054ed67f3a91232b47bcb2f20e4))
* **community:** upgrade community page into a full open-source hub ([#35](https://github.com/sunormesky-max/epicode/issues/35)) ([703ba6a](https://github.com/sunormesky-max/epicode/commit/703ba6a2e251cd61798d1cce771417f93d29d8bf))
* complete open-source community ecosystem ([6bfb60d](https://github.com/sunormesky-max/epicode/commit/6bfb60da352ffdafeb7fe5159e99d02c4bac19b1))
* enhance community engagement infrastructure ([6bc382a](https://github.com/sunormesky-max/epicode/commit/6bc382a6aee09208364a753bdadf05fdf0a85faa))
* open-source Sprint 1 — version unification, repo cleanup, README fix, API prefix standardization ([4a40c2a](https://github.com/sunormesky-max/epicode/commit/4a40c2afc1f5e380d9c874fe19d004b2f08ecb41))
* Sprint 2 — CI matrix expansion, community health updates, pre-commit, dependabot hardening ([0087613](https://github.com/sunormesky-max/epicode/commit/00876138b6c97bd54c71a7bf7b106b21f9967bee))
* Sprint 3 — complete all remaining P1/P2 items, no loose ends ([248792a](https://github.com/sunormesky-max/epicode/commit/248792ae776e54261b9e60415408f7f43c663dbd))


### Bug Fixes

* **backend:** address P0 audit findings ([93c4308](https://github.com/sunormesky-max/epicode/commit/93c43082cfa596efb396c4ca08d493fe2ab53520))
* **ci:** make CI green — clippy, rustfmt, eslint, and docker build ([#36](https://github.com/sunormesky-max/epicode/issues/36)) ([067bace](https://github.com/sunormesky-max/epicode/commit/067bace1754695a899e3d100040afb30e3fcc8e4))
* **ci:** run tests with single thread to avoid DB lock conflicts ([18721da](https://github.com/sunormesky-max/epicode/commit/18721da2e7533b9f728bdb895d5fefe0993d19e0))
* **codeql:** disable Rust analysis due to initialization failures ([08e2be1](https://github.com/sunormesky-max/epicode/commit/08e2be11e3f356150f2109ed1edb5c7a4493671e))
* **deploy:** add /stats/public to nginx proxy routes ([8febf1c](https://github.com/sunormesky-max/epicode/commit/8febf1c69cded32e2c97ef5692eb0327a313a4ee))
* **docker:** remove model file copy from Dockerfile.cloud ([cbe37b4](https://github.com/sunormesky-max/epicode/commit/cbe37b45ca2e5cab0531cb1c72c359fd3688ee02))
* **frontend:** add real GitHub links to navbar and footer ([da055d7](https://github.com/sunormesky-max/epicode/commit/da055d73b377fe0b94f638cf192935c7d9b51634))
* **readme:** correct deprecation note wording ([9a892c3](https://github.com/sunormesky-max/epicode/commit/9a892c3f63235540971adb77a2f11af15d366e44))
* **readme:** move SDK section to correct position ([bb5cad5](https://github.com/sunormesky-max/epicode/commit/bb5cad512b61f307a49231b134bdd8ac620b0e6d))
* **release:** overhaul release workflow ([30fef13](https://github.com/sunormesky-max/epicode/commit/30fef133844cda7b91efb51e47a5ee9553b09f93))
* remove accidentally created root-level package files ([1fe6040](https://github.com/sunormesky-max/epicode/commit/1fe6040ecf48aac365692b77606504b3a73d5f2c))
* remove needless borrows for clippy warnings ([4c62e60](https://github.com/sunormesky-max/epicode/commit/4c62e6069f0ab7f9d79354807a0eb677cfd2af37))
* resolve CI failures - rustfmt and unused import ([cce5acb](https://github.com/sunormesky-max/epicode/commit/cce5acb913cb89268e1a62e662d141d87e3089bd))
* **security:** harden deployment — body limit, non-root container, pinned images ([#38](https://github.com/sunormesky-max/epicode/issues/38)) ([7ffd89d](https://github.com/sunormesky-max/epicode/commit/7ffd89d66096671a6af844015e78cf4eeb50f27d))
* **workflows:** fix broken GitHub Actions workflows ([3a0ccde](https://github.com/sunormesky-max/epicode/commit/3a0ccde567386569564957bfabdf108abf6f8a58))


### Documentation

* add AUTHORS.md for community recognition ([5689966](https://github.com/sunormesky-max/epicode/commit/568996637f9539c81459093b3064466293fec03c))
* standardize README, release flow, and community docs ([1d430e4](https://github.com/sunormesky-max/epicode/commit/1d430e44818d746454be86400b3a7d1b24d1d475))
* update CHANGELOG and ROADMAP for community release ([1706421](https://github.com/sunormesky-max/epicode/commit/17064210b2b0263ccc689d06cfc4b81b956bf9a2))


### Code Refactoring

* **sdk:** rename SDK from tetramem-sdk to epicode-sdk ([8841bc4](https://github.com/sunormesky-max/epicode/commit/8841bc4796b20c37151d813e9164dd9c8b9bf644))


### Security

* harden HTML sanitization, CORS, and rate limiting ([9abfdf5](https://github.com/sunormesky-max/epicode/commit/9abfdf5610b76dc8293cdbc76af9c1389602c528))
* update frontend deps to fix 9 npm vulnerabilities ([1767c8b](https://github.com/sunormesky-max/epicode/commit/1767c8b1b572f5ddd7bf5ae96ccdf15726582102))

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
