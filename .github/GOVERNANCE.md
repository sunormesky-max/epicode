# Epicode Governance

This document outlines the governance structure for the Epicode open-source project. It defines roles, responsibilities, decision-making processes, and how community members can participate and advance within the project.

## Project Roles

### Community Members

Anyone who uses, contributes to, or participates in discussions about Epicode is a community member. Community members are encouraged to:

- Open issues to report bugs, request features, or ask questions
- Participate in discussions and provide feedback
- Contribute code, documentation, or tests via pull requests
- Help other community members in discussions and issue threads
- Share the project and help it grow

### Contributors

Contributors are community members who have made substantive contributions to the project. This includes code contributions, documentation improvements, bug reports with detailed analysis, or sustained community support.

Contributors are recognized through:
- Attribution in release notes and changelogs
- Listing in the project contributors documentation
- Eligibility for consideration as a maintainer after sustained contribution

### Maintainers

Maintainers are trusted contributors with additional responsibilities for the project's direction and quality. They have write access to the repository and are responsible for:

- Reviewing and merging pull requests
- Triaging issues and prioritizing work
- Enforcing the code of conduct
- Making architectural and technical decisions
- Mentoring contributors and helping them grow
- Cutting releases and managing the changelog
- Maintaining project infrastructure (CI/CD, dependencies, etc.)

Current maintainers are listed in the project README and can be contacted via GitHub issues or discussions.

## Decision Making Process

### Day-to-Day Decisions

Most decisions are made through the normal pull request process. Maintainers review and merge contributions that align with the project's goals and quality standards. When a maintainer is unsure about a change, they should seek input from other maintainers or the community.

### Significant Decisions

For significant changes that affect the project's architecture, public API, or long-term direction, the following process applies:

1. **Proposal**: A maintainer or contributor opens a GitHub Discussion or issue with a detailed proposal
2. **Feedback Period**: The community has at least one week to provide feedback (longer for major changes)
3. **Consensus Building**: Maintainers work to build consensus among contributors and the community
4. **Decision**: If consensus is reached, the proposal is accepted. If not, a majority vote among maintainers decides
5. **Documentation**: The decision and rationale are documented in the relevant issue or discussion

### Breaking Changes

Breaking changes to the public API require:
- A clear proposal with migration path
- Approval from at least two maintainers
- Documentation in the changelog under the "Changed" or "Removed" category
- A deprecation period when possible (at least one minor release)

## How to Become a Maintainer

The project welcomes new maintainers who have demonstrated commitment and competence. The path to becoming a maintainer is:

### 1. Sustained Contribution

Make meaningful contributions over a period of time (typically 3-6 months). This includes:
- High-quality code contributions
- Thoughtful code reviews on others' pull requests
- Documentation improvements
- Issue triage and community support

### 2. Nomination

An existing maintainer nominates a contributor by opening a private discussion with other maintainers. The nomination should include:
- Summary of the nominee's contributions
- Assessment of their technical judgment and communication skills
- Evidence of alignment with project values

### 3. Maintainer Vote

Existing maintainers vote on the nomination. A simple majority is required for approval. The vote is typically held privately but the outcome is announced publicly.

### 4. Onboarding

New maintainers are onboarded with:
- Write access to the repository
- Introduction to the team and existing processes
- Guidance on responsibilities and expectations
- A mentor maintainer for the first few months

### Maintainer Expectations

Maintainers are expected to:
- Act in the best interest of the project and community
- Be responsive to issues, pull requests, and discussions
- Participate in decision-making processes
- Uphold and enforce the code of conduct
- Maintain confidentiality when handling sensitive reports
- Step down gracefully if they can no longer fulfill their duties

## Code of Conduct Enforcement

Maintainers are responsible for enforcing the [Code of Conduct](../CODE_OF_CONDUCT.md). The enforcement process is designed to be fair, transparent, and protective of the community.

### Reporting

Reports of unacceptable behavior can be submitted:
- Via GitHub issues (for public, non-sensitive reports)
- Via GitHub security advisories (for sensitive matters)
- Via email to the maintainers at the address listed in the Code of Conduct

### Investigation

When a report is received:
1. A maintainer acknowledges receipt within 48 hours
2. The report is reviewed by at least two maintainers (or all if the team is small)
3. The subject of the report is given an opportunity to respond
4. Maintainers gather relevant context and evidence

### Resolution

Possible resolutions include:
- **No action**: If the behavior is determined to be acceptable or a misunderstanding
- **Warning**: A private warning about the behavior and expectations going forward
- **Temporary ban**: Removal from project spaces for a specified period
- **Permanent ban**: Removal from project spaces indefinitely

For serious violations (harassment, threats, illegal activity), immediate action may be taken without the full process, subject to later review.

### Appeals

Individuals who disagree with an enforcement decision may appeal by contacting the maintainers who were not involved in the original decision. Appeals should include new information or a clear explanation of why the decision was incorrect.

### Transparency

Enforcement actions are documented privately. Annual summaries (without identifying details) may be shared with the community to maintain accountability.

## Project Leadership Structure

### Lead Maintainer

The project has a Lead Maintainer who serves as the final decision-maker when consensus cannot be reached and represents the project in external matters. The Lead Maintainer is:

- Selected by consensus among maintainers
- Responsible for setting the overall project vision and roadmap
- The default point of contact for external organizations and media
- Accountable for the health and sustainability of the project

### Working Groups

For specific areas of the project, working groups may be formed. These are informal groups of contributors and maintainers focused on:
- Documentation and community outreach
- Security and vulnerability response
- Performance and optimization
- Specific modules or features

Working groups are self-organizing and report to the maintainers as a whole.

### Succession

If the Lead Maintainer steps down or becomes inactive:
1. They are encouraged to designate a successor
2. If no successor is designated, maintainers vote to select a new Lead Maintainer
3. The transition is documented and communicated to the community

## Changes to Governance

Changes to this governance document require:
- A proposal opened as a GitHub Discussion
- A feedback period of at least two weeks
- Approval by a two-thirds majority of maintainers
- Documentation of the change in this file with a changelog entry

---

This governance model is designed to be lightweight and adaptable. As the project grows, we may evolve these structures to better serve the community. Feedback on governance is always welcome.
