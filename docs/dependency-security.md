# Dependency audit policy

CI scans these maintained dependency scopes:

| Scope | Scanner and blocking rule |
| --- | --- |
| `backend/Cargo.lock`, `guard/Cargo.lock` | Pinned `cargo-audit` JSON; vulnerabilities and unsound advisories block. Unmaintained/notice warnings are reported but non-blocking. |
| `frontend/package-lock.json`, `backend/sdk/typescript/package-lock.json` | `npm audit` JSON; high and critical advisories block. Lower severities remain visible, and Dependency Review blocks moderate-or-higher newly introduced by a PR. |
| `backend/sdk/python/pyproject.toml` runtime dependencies, `mcp-bridge/requirements.txt` | `pip-audit --strict`; any finding blocks because its JSON report does not provide severity. |

`scripts/dependency-audit-policy.json` is the sole exception list. Each exception must name one manifest scope, advisory ID, exact package and locked version, owner, rationale, and expiry date. CI rejects mismatches, expired or unused exceptions, malformed scanner output, and undeclared scopes. Exceptions are not renewed automatically; an extension needs maintainer review in a PR. Do not add CLI `--ignore` flags or text-based output filters.

Current residual findings:

- `RUSTSEC-2023-0071` affects `rsa@0.9.10` (CVSS 5.9 medium), with no patched release; RustCrypto tracks the constant-time remediation in issue #626. The service uses RSA for client-public-key encryption; the private-key decrypt helper has no HTTP/MCP call site and is exercised only by tests. The exact exception expires 2026-11-09.
- `GHSA-vfj7-8cjw-p6xm` affects `braces@3.0.3` (high) in the frontend's dev-only Tailwind 3.4.19 build chain. No patched braces 3.x release is available; the proposed Tailwind 4 migration is a major upgrade. The exact exception expires 2026-11-09.
- `GHSA-rj75-hqrm-r3gf` affects `postcss-selector-parser@6.1.4` (moderate). It is reported but does not cross the npm blocking threshold.

Previous Rust ignores reviewed:

- `RUSTSEC-2025-0141` (`bincode`) is absent from both Rust lockfiles, so its ignore was removed.
- `RUSTSEC-2026-0190` affected `anyhow@1.0.102`; `backend/Cargo.lock` now pins patched `anyhow@1.0.103`.
- `RUSTSEC-2026-0285` affected Guard's `rustls@0.23.41`; `guard/Cargo.lock` now pins patched `rustls@0.23.45` (and `rustls-webpki@0.103.15`).
- `RUSTSEC-2024-0436` (`paste@1.0.15`) is reachable via `tokenizers`; its upstream repository is archived and no patch is available (the advisory lists `pastey` and `with_builtin_macros` alternatives). `RUSTSEC-2025-0119` (`number_prefix@0.4.0`) is reachable via `indicatif`/`tokenizers`, is unmaintained, and has no patched release (the advisory suggests `unit-prefix`). Neither is a vulnerability advisory; both remain visible but non-blocking.
