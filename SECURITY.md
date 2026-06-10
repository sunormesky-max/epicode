# Security Policy

## Reporting a Vulnerability

We take security vulnerabilities seriously. If you discover a security issue in Epicode, please report it responsibly.

### Do NOT:

- Open a public GitHub issue for the vulnerability
- Exploit the vulnerability beyond what is necessary to demonstrate it
- Share the vulnerability with others before it has been addressed

### DO:

**Report via GitHub Security Advisories (preferred):**

Go to [https://github.com/sunormesky-max/epicode/security/advisories/new](https://github.com/sunormesky-max/epicode/security/advisories/new) and submit a private vulnerability report.

### What to Include

1. **Description** — Clear description of the vulnerability
2. **Impact** — What an attacker could achieve
3. **Reproduction** — Step-by-step instructions
4. **Proof of Concept** — If applicable
5. **Suggested Fix** — If you have one

### Response Timeline

| Stage | Timeline |
|-------|----------|
| Acknowledgment | Within 48 hours |
| Initial Assessment | Within 7 days |
| Fix Development | Depends on severity |
| Disclosure | After fix is released |

## Supported Versions

| Version | Supported |
|---------|-----------|
| 1.0.x | Yes |

## Security Best Practices for Deployment

If you are self-hosting Epicode:

1. **Always set environment variables** — never use placeholder values
2. **Use strong, unique keys** — generate with `openssl rand -base64 32`
3. **Enable HTTPS** — use a reverse proxy (Nginx/Caddy) with TLS
4. **Restrict network access** — bind to `127.0.0.1`, use firewall rules
5. **Keep dependencies updated** — run `cargo audit` regularly
6. **Rotate keys periodically** — especially if you suspect exposure
7. **Review access logs** — monitor for suspicious activity

## Security Features

Epicode includes the following security measures:

- AES-256-GCM encryption for stored data
- Argon2id password hashing
- Constant-time key comparison
- API key authentication
- Rate limiting
- Security headers (CSP, HSTS, X-Frame-Options)
- Login brute-force protection (account lockout after 5 failures)
