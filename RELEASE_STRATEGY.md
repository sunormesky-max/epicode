# Release & Version Sync Strategy

## 1. Purpose

This document defines the release cadence, feature declassification pipeline, and version synchronization rules between the production (internal) and open-source (public) codebases.

## 2. Stable Release Cadence

| Release Type | Cadence | Description |
|-------------|---------|-------------|
| **Stable** | Monthly | A numbered stable release (`v{year}.{month}.0`) is cut on the first Monday of every month. |
| **Hotfix** | As needed | Critical patches are cherry-picked onto the latest stable branch and released as `v{year}.{month}.{patch}`. |
| **Preview** | Bi-weekly | Optional preview tags (`v{year}.{month}.0-preview.{n}`) may be published for early adopters. |

### Calendar Example

```
Jan 5  → v2026.1.0
Feb 2  → v2026.2.0
Mar 2  → v2026.3.0
...
```

## 3. Production ↔ Open Source Relationship

| Dimension | Production (Internal) | Open Source (Public) |
|-----------|----------------------|----------------------|
| **Source of truth** | Internal monorepo | Public GitHub repository |
| **Versioning** | `prod-v{year}.{month}.{patch}` | `v{year}.{month}.{patch}` |
| **Access** | Employees + authorized contractors | Community + external contributors |
| **Features** | All features (including restricted) | Declassified features only |
| **Support** | Enterprise SLA | Community-driven (GitHub Issues / Discussions) |

## 4. Feature Graduation Pipeline

All production features must pass through the following pipeline before appearing in the open-source release:

```
┌─────────────┐     ┌─────────────┐     ┌─────────────────┐     ┌─────────────┐
│  Production │ ──▶ │  Validation │ ──▶ │ Declassification │ ──▶ │ Open Source │
│   (source)  │     │   (1 week)  │     │   (1–2 weeks)   │     │  (release)  │
└─────────────┘     └─────────────┘     └─────────────────┘     └─────────────┘
```

### 4.1 Stages

| Stage | Owner | Duration | Criteria |
|-------|-------|----------|----------|
| **Production** | Engineering team | N/A | Feature is merged to `main` and deployed internally. |
| **Validation** | QA + Security | 1 week | Functional tests, security review, and performance benchmarks pass. |
| **Declassification** | Legal + Product | 1–2 weeks | Remove proprietary dependencies, scrub internal identifiers, and redact restricted business logic. |
| **Open Source** | Release engineer | 1 day | Cherry-pick declassified commits to public `main`, tag release, and publish release notes. |

### 4.2 Declassification Checklist

- [ ] Remove internal API keys, tokens, and endpoints.
- [ ] Replace proprietary ML models or datasets with open equivalents or stubs.
- [ ] Scrub internal Jira/Linear ticket references from commit messages.
- [ ] Ensure no hard-coded employee emails or internal hostnames.
- [ ] Verify license compatibility of all dependencies.
- [ ] Update public documentation to reflect the feature.

## 5. Version Sync Status

| Component | Production Version | Open Source Version | Sync Status | Last Sync |
|-----------|-------------------|---------------------|-------------|-----------|
| Core runtime | `prod-v2026.6.2` | `v2026.6.2` | ✅ Synced | 2026-06-24 |
| Agent SDK | `prod-v2026.6.1` | `v2026.6.1` | ✅ Synced | 2026-06-20 |
| CLI tools | `prod-v2026.6.2` | `v2026.6.1` | ⚠️ Pending | 2026-06-20 |
| Security policies | `prod-v2026.6.2` | `v2026.5.0` | 🔒 Restricted | N/A |
| Dashboard UI | `prod-v2026.6.2` | `v2026.6.0` | ⚠️ Pending | 2026-06-10 |

## 6. Communication Channels

| Channel | Audience | Purpose |
|---------|----------|---------|
| **GitHub Releases** | Public community | Automated release notes, changelogs, and binary downloads. |
| **#announcements (Slack)** | Internal staff | Production deployment notices and incident alerts. |
| **Discussions (GitHub)** | External contributors | RFCs, roadmap feedback, and release Q&A. |
| **Monthly newsletter** | Users + stakeholders | High-level summary of shipped features and upcoming changes. |
| **CHANGELOG.md** | All readers | Authoritative, line-by-line record of every merged change. |

## 7. Responsibilities

| Role | Responsibility |
|------|----------------|
| **Release Engineer** | Cuts tags, manages cherry-picks, and verifies CI/CD pipelines. |
| **Product Manager** | Decides which features enter the declassification queue. |
| **Security Lead** | Approves or blocks features based on data-classification review. |
| **Open Source Maintainer** | Reviews external PRs, triages issues, and publishes release notes. |

## 8. Escalation & Exceptions

1. **Emergency hotfix:** If a critical vulnerability is discovered, the Release Engineer may bypass the Validation stage with written approval from the Security Lead.
2. **Delayed declassification:** If Legal review exceeds 2 weeks, the Product Manager may schedule the feature for the next release cycle rather than blocking the current one.
3. **Restricted features:** Features marked `CONFIDENTIAL` or `INTERNAL ONLY` are permanently excluded from the open-source pipeline unless explicitly reclassified by the Security Lead.

---

*Last updated: 2026-06-24*  
*Next review: 2026-07-24*
