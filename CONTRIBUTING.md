# Contributing to Epicode

Thanks for helping improve Epicode. This repository aims to be a normal, healthy open-source project: changes should be small, testable, documented, and reviewable.

## Before you start

- Read the [README](README.md), [Security Policy](SECURITY.md), [Code of Conduct](CODE_OF_CONDUCT.md), and [Roadmap](ROADMAP.md).
- Search existing [issues](https://github.com/sunormesky-max/epicode/issues) and [discussions](https://github.com/sunormesky-max/epicode/discussions) before opening a new one.
- Prefer one PR per focused fix or feature.

## Development setup

### Prerequisites

- Rust 1.88+
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

### Running the full stack locally

```bash
# Terminal 1 — backend Cloud mode
cd backend
cargo build --release
export TETRAMEM_API_KEY="$(openssl rand -base64 32)"
export DEEPSEEK_API_KEY="your-deepseek-key"
./target/release/epicode --cloud

# Terminal 2 — frontend dev server
cd frontend
npm install
npm run dev
```

Visit `http://localhost:5173` for the dashboard and `http://localhost:9111` for the Cloud API.

## Commit style

We use [Conventional Commits](https://www.conventionalcommits.org/) so that releases can be automated.

| Type | When to use | Release impact |
|------|-------------|----------------|
| `feat:` | New feature | MINOR bump |
| `fix:` | Bug fix | PATCH bump |
| `docs:` | Documentation only | No bump |
| `style:` | Formatting, no logic change | No bump |
| `refactor:` | Code change that is neither feat nor fix | No bump |
| `perf:` | Performance improvement | PATCH bump |
| `test:` | Adding or fixing tests | No bump |
| `chore:` | Build, deps, tooling | No bump |
| `feat!:` or `BREAKING CHANGE:` | Backward-incompatible change | MAJOR bump |

Example:

```bash
git commit -m "feat(scheduler): add memory eviction based on quality score"
```

## Pull request workflow

1. Fork the repository and create a branch.
2. Make the smallest change that solves the problem.
3. Add or update tests when behavior changes.
4. Update docs if user-facing behavior changes.
5. Fill out the pull request template.
6. Ensure CI is green.

## Code review

- All PRs must pass the full CI suite.
- At least one maintainer review is required before merging.
- Force-pushing after review has started is discouraged; prefer follow-up commits.

## Issue labels

| Label | Use for |
|-------|---------|
| `bug` | Confirmed bugs |
| `enhancement` | Feature requests |
| `good first issue` | Friendly entry points for new contributors |
| `help wanted` | Issues where community help is especially welcome |
| `security` | Security-related concerns |
| `documentation` | Docs improvements |

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
