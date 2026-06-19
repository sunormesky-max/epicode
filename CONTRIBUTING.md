# Contributing to Epicode

Thanks for helping improve Epicode. This repository aims to be a normal, healthy open-source project: changes should be small, testable, documented, and reviewable.

## Before you start

- Read the [README](README.md), [Security Policy](SECURITY.md), and [Code of Conduct](CODE_OF_CONDUCT.md).
- Search existing [issues](https://github.com/sunormesky-max/epicode/issues) and [discussions](https://github.com/sunormesky-max/epicode/discussions) before opening a new one.
- Prefer one PR per focused fix or feature.

## Development setup

### Prerequisites

- Rust 1.85+
- Node.js 20+
- SQLite

### Backend

```bash
cd backend
cargo fmt --all
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
```

### Frontend

```bash
cd frontend
npm ci
npm run check
npm run lint
npm run test
npm run build
```

## Pull request workflow

1. Fork the repository and create a branch.
2. Make the smallest change that solves the problem.
3. Add or update tests when behavior changes.
4. Update docs if the user-facing behavior changes.
5. Fill out the pull request template.

## Code style

- Rust: `cargo fmt` and `cargo clippy`
- TypeScript: existing ESLint/Prettier rules
- Do not add secrets, credentials, or placeholder backdoors
- Keep public APIs and docs aligned

## Reporting issues

When filing a bug, include:

- version or commit
- environment
- exact steps to reproduce
- expected vs actual behavior
- logs or screenshots if helpful

For feature requests, include:

- the problem you want to solve
- your proposed solution
- any alternatives you considered

## Security reports

Report vulnerabilities privately through [Security Advisories](SECURITY.md).

## License

By contributing, you agree that your changes will be licensed under [MIT](LICENSE).
