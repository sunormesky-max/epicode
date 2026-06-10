# Contributing to Epicode

Thank you for your interest in contributing to Epicode! This guide will help you get started.

## Quick Links

- [Report a Bug](https://github.com/sunormesky-max/epicode/issues/new?template=bug_report.md)
- [Request a Feature](https://github.com/sunormesky-max/epicode/issues/new?template=feature_request.md)
- [Ask a Question](https://github.com/sunormesky-max/epicode/discussions)
- [Security Vulnerability](SECURITY.md)

## Development Setup

### Prerequisites

- Rust 1.85+
- Node.js 18+ (for frontend)
- SQLite3
- ONNX model files (see `backend/deploy/deploy.sh` for download URLs)

### Backend

```bash
cd backend
cargo build
cargo test --lib
```

### Frontend

```bash
cd frontend
npm install
npm run dev
```

## How to Contribute

### 1. Fork and Clone

```bash
git clone https://github.com/YOUR_USERNAME/epicode.git
cd epicode
```

### 2. Create a Branch

```bash
git checkout -b feature/your-feature-name
```

### 3. Make Your Changes

- Follow existing code style and conventions
- Add tests for new functionality
- Update documentation if needed

### 4. Commit

Use clear, descriptive commit messages:

```
feat: add batch delete API endpoint
fix: resolve memory leak in WebSocket handler
docs: update API reference for search endpoint
```

### 5. Push and Create PR

```bash
git push origin feature/your-feature-name
```

Then open a Pull Request against the `main` branch.

## Pull Request Guidelines

- **One PR per feature/fix** — keep changes focused
- **Describe what and why** — explain the motivation, not just the implementation
- **Include tests** — new code should be tested
- **Keep diffs small** — easier to review, faster to merge
- **Update docs** — if you change behavior, update relevant documentation

## Code Style

### Rust (Backend)

- Follow `cargo fmt` and `cargo clippy` recommendations
- Use `Result<T, String>` for error handling in business logic
- Add `#[cfg(test)]` modules for unit tests
- Document public APIs with `///` doc comments

### TypeScript (Frontend)

- Follow existing ESLint configuration
- Use functional components with hooks
- Keep components small and focused
- Use `useMemo` for expensive computations

## Reporting Issues

### Bug Reports

Please include:

1. Epicode version
2. Operating system
3. Steps to reproduce
4. Expected vs actual behavior
5. Relevant logs

### Feature Requests

Please describe:

1. The problem you're trying to solve
2. Your proposed solution
3. Any alternatives you've considered

## License

By contributing to Epicode, you agree that your contributions will be licensed under the [AGPL-3.0-or-later](LICENSE) license.

## Questions?

Open a [Discussion](https://github.com/sunormesky-max/epicode/discussions) for general questions, ideas, or feedback.
