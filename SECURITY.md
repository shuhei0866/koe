# Security Policy

## Reporting a Vulnerability

If you discover a security vulnerability in koe, please report it responsibly.

**Do NOT open a public GitHub issue for security vulnerabilities.**

Instead, please email: **shuhei0866@gmail.com**

Include:
- Description of the vulnerability
- Steps to reproduce
- Potential impact
- Suggested fix (if any)

You should receive a response within 48 hours. We will work with you to understand and address the issue before any public disclosure.

## Supported Versions

| Version | Supported |
|---------|-----------|
| latest  | Yes       |
| < latest | No      |

## Security Measures

- All dependencies are audited weekly via `cargo audit`
- GitHub Actions use SHA-pinned actions (not mutable tags)
- Builds use `--locked` to ensure reproducible builds from `Cargo.lock`
- Release binaries include SHA256 checksums
