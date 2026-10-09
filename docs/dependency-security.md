# Dependency audit policy

CI scans these maintained dependency scopes:

| Scope | Scanner and blocking rule |
| --- | --- |
| `backend/Cargo.lock`, `guard/Cargo.lock` | Pinned `cargo-audit` JSON; vulnerabilities and unsound advisories block. Unmaintained/notice warnings are reported but non-blocking. |
| `frontend/package-lock.json`, `backend/sdk/typescript/package-lock.json` | `npm audit` JSON; high and critical advisories block. Lower severities remain visible, and Dependency Review blocks moderate-or-higher newly introduced by a PR. |
| `backend/sdk/python/pyproject.toml` runtime dependencies, `mcp-bridge/requirements.txt` | `pip-audit --strict`; any finding blocks because its JSON report does not provide severity. |

`scripts/dependency-audit-policy.json` is the sole exception list and is currently empty. Any future exception must name one manifest scope, advisory ID, exact package and locked version, owner, rationale, and expiry date. CI rejects mismatches, expired or unused exceptions, malformed scanner output, and undeclared scopes. Exceptions are not renewed automatically; an extension needs maintainer review in a PR. Do not add CLI `--ignore` flags or text-based output filters.

Current residual findings:

- `GHSA-vfj7-8cjw-p6xm` affects `braces@3.0.3` (high) in the frontend's dev-only Tailwind 3.4.19 build chain. No patched braces 3.x release is available; no scanner exception remains, so CI blocks on this finding.
- `GHSA-rj75-hqrm-r3gf` affects `postcss-selector-parser@6.1.4` (moderate). It is reported but does not cross the npm blocking threshold.
- `paste@1.0.15` and `number_prefix@0.4.0` remain informational unmaintained notices in the backend dependency tree; neither is a vulnerability advisory.

Previous Rust ignores reviewed:

- `RUSTSEC-2025-0141` (`bincode`) is absent from both Rust lockfiles, so its ignore was removed.
- `RUSTSEC-2023-0071` affected `rsa@0.9.10` (CVSS 5.9 medium, no patched release). This PR removes `rsa`, uses AWS-LC for the existing RSA-OAEP-SHA256 public-encryption format, and compiles the private-decrypt test helper only for tests; no exception remains.
- `RUSTSEC-2026-0190` affected `anyhow@1.0.102`; `backend/Cargo.lock` now pins patched `anyhow@1.0.103`.
- `RUSTSEC-2026-0285` affected Guard's `rustls@0.23.41`; `guard/Cargo.lock` now pins patched `rustls@0.23.45` (and `rustls-webpki@0.103.15`).
- `RUSTSEC-2024-0436` (`paste@1.0.15`) is reachable via `tokenizers`; its upstream repository is archived and no patch is available (the advisory lists `pastey` and `with_builtin_macros` alternatives). `RUSTSEC-2025-0119` (`number_prefix@0.4.0`) is reachable via `indicatif`/`tokenizers`, is unmaintained, and has no patched release (the advisory suggests `unit-prefix`). Neither is a vulnerability advisory; both remain visible but non-blocking.

## Tailwind remediation decision

`npm audit fix --force --dry-run` proposes Tailwind 4.3.3 as the only available fix for the high `braces` advisory. Tailwind's official migration guide sets minimum browser versions of Safari 16.4, Chrome 111, and Firefox 128 and requires migrating the PostCSS plugin and v3 CSS directives/configuration. This repository has no declared browser-support baseline and uses the v3 setup. The PR has no exception and remains draft until maintainers choose to approve that browser-floor/toolchain migration, approve a vetted Tailwind 3-compatible patched dependency source, or wait for an upstream braces 3.x fix.
