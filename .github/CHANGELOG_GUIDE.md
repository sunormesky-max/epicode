# Changelog Guide

This guide documents how to maintain the Epicode changelog. A well-maintained changelog helps users and contributors understand what has changed between releases and makes upgrading easier.

We follow the principles of [Keep a Changelog](https://keepachangelog.com/) and use categories based on [Semantic Versioning](https://semver.org/) impact.

## Changelog Format

The changelog is maintained in `CHANGELOG.md` at the root of the repository. Each release has a section with the following structure:

```markdown
## [Version] - YYYY-MM-DD

### Category

- Description of the change ([#PR](link) by [@author](link))
```

### Example Entry

```markdown
## [1.2.0] - 2026-06-21

### Added

- Support for custom output templates in code generation ([#142](https://github.com/sunorme/sunormesky-max_epicode/pull/142) by [@contributor](https://github.com/contributor))

### Fixed

- Corrected parsing of nested block comments in TypeScript files ([#138](https://github.com/sunorme/sunormesky-max_epicode/issues/138))
```

## Categories

Every changelog entry must fall under one of these categories:

### `Added`
New features, capabilities, or functionality.

Examples:
- New CLI flags or configuration options
- New language or framework support
- New APIs or public methods
- New documentation sections

### `Changed`
Changes to existing functionality that are not bug fixes.

Examples:
- Behavioral changes to existing features
- Updated dependencies
- Refactored internals that affect performance
- UI or output formatting changes

### `Deprecated`
Features that are still available but will be removed in a future release.

Examples:
- APIs marked for removal
- CLI flags replaced by alternatives
- Configuration options that will change

Always include a note about when the feature will be removed, if known.

### `Removed`
Features that were removed in this release.

Examples:
- Deleted APIs or methods
- Removed CLI flags
- Dropped support for older language versions or platforms

### `Fixed`
Bug fixes and corrections to existing functionality.

Examples:
- Crashes or exceptions
- Incorrect output or behavior
- Race conditions or concurrency issues
- Documentation corrections for inaccurate information

### `Security`
Vulnerability fixes and security improvements.

Examples:
- Patches for CVEs or security advisories
- Fixes for injection or escape vulnerabilities
- Improvements to authentication or authorization

Security entries should be prominent and may include a reference to the CVE or advisory.

## Writing Good Changelog Entries

### Be Specific

Describe what changed and why it matters to users. Avoid vague entries like "fixed bugs" or "improved code."

**Bad:**
```
- Fixed bug
```

**Good:**
```
- Fixed crash when parsing files with empty function bodies ([#145](https://github.com/sunorme/sunormesky-max_epicode/issues/145))
```

### Focus on User Impact

Write from the perspective of someone upgrading the project. What do they need to know? What might break? What new capabilities do they gain?

**Bad:**
```
- Refactored parser module
```

**Good:**
```
- Improved parsing speed by 30% for large TypeScript files through optimized AST traversal
```

### Use Imperative Mood

Start entries with a verb in the imperative mood ("Add", "Fix", "Remove", not "Added", "Fixed", "Removed"). This matches commit message conventions and keeps entries concise.

**Bad:**
```
- Added support for Python 3.12
```

**Good:**
```
- Add support for Python 3.12
```

### Link to PRs and Issues

Every entry should reference the pull request that introduced the change, and the related issue if one exists. This provides context and makes it easy to trace the full history.

Format:
- For PRs: `([#PR](link) by [@author](link))`
- For issues: `([#Issue](link))`
- For both: `([#Issue](link), [#PR](link) by [@author](link))`

### Group Related Entries

When multiple changes relate to the same feature or area, group them together or consider combining into a single descriptive entry.

### Deprecation and Removal Notices

When deprecating or removing features, include:
- What is being deprecated or removed
- What should be used instead (migration path)
- When the feature will be or was removed

Example:
```markdown
### Deprecated

- The `--old-flag` CLI option is deprecated and will be removed in v2.0. Use `--new-flag` instead. ([#156](https://github.com/sunorme/sunormesky-max_epicode/pull/156))
```

## Release Process

### Before Release

1. Ensure all merged changes since the last release are documented
2. Verify categories are correct and entries are well-written
3. Add the release version and date header
4. Update the `[Unreleased]` section to be empty

### Unreleased Section

The top of the changelog should always have an `[Unreleased]` section for changes that have been merged but not yet released:

```markdown
## [Unreleased]

### Added

### Changed

### Fixed

### Security
```

This section is updated as changes are merged and is converted into a versioned release section when a release is cut.

### Versioning

We follow [Semantic Versioning](https://semver.org/):

- **MAJOR** (X.y.z): Breaking changes that require user action
- **MINOR** (x.Y.z): New features, backwards compatible
- **PATCH** (x.y.Z): Bug fixes and security patches, backwards compatible

## Formatting Conventions

- Use Markdown format with `##` for release headers and `###` for category headers
- List entries as bullet points with `-`
- Use backticks for code, flags, file names, and API references
- Keep lines under 100 characters when possible
- Order categories as: Added, Changed, Deprecated, Removed, Fixed, Security
- Order entries within a category by significance (most impactful first)
- Use ISO 8601 date format: `YYYY-MM-DD`

## Automation

Where possible, changelog entries should be added in the pull request that introduces the change. Maintainers will review and edit entries during code review.

For releases, maintainers may use automation tools to generate draft changelogs from merged PRs, but the final changelog should always be reviewed and edited by a human for clarity and accuracy.
