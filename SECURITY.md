# Security Policy

## Supported Versions

The following versions of Epicode are currently supported with security updates:

| Version | Supported          |
| ------- | ------------------ |
| 0.1.x   | :white_check_mark: |
| < 0.1   | :x:                |

## Reporting a Vulnerability

If you discover a security vulnerability in Epicode, please report it responsibly:

1. **Do NOT** open a public issue
2. Send an email to security@epicode.cn with:
   - A description of the vulnerability
   - Steps to reproduce (if applicable)
   - Possible impact assessment
   - Suggested fix (if any)

We will acknowledge receipt within 48 hours and provide a timeline for resolution.

## Security Measures

Epicode implements the following security measures:

- **Encryption at rest**: All data is encrypted using AES-256-GCM
- **Secure memory**: Sensitive data is stored in locked memory pages
- **Constant-time operations**: Cryptographic operations use constant-time implementations
- **Input validation**: All user inputs are validated and sanitized
- **Dependency scanning**: Regular automated scans for vulnerable dependencies

## Security Features

- Master key encryption with hardware security module support
- Memory protection with mlock/mprotect
- Secure key derivation using Argon2id
- Audit logging for all sensitive operations

## Acknowledgments

We thank all security researchers who have responsibly disclosed vulnerabilities.
