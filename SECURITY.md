# Security policy

## Reporting

Email connersalt123@outlook.com with a description and reproduction steps, or open a private security advisory on GitHub. Please do not file public issues for vulnerabilities. Expect an acknowledgement within a few days; fixes ship as a patch release with a CHANGELOG entry.

## Scope

Mimic is a local desktop application. The attack surface is:

- the loopback HTTP bridge the Lightroom plugin talks to (`127.0.0.1`, per-launch bearer token, bounded bodies, no browser origins);
- the Python engine child process (argument-array spawn, NDJSON over stdio, message size caps, structured errors, no shell);
- the auto-updater (Tauri updater with signature verification against the embedded public key; private key only in CI secrets);
- files the user points Mimic at (opened read-only; malformed sidecars are isolated per file).

Details and threat model: [docs/SECURITY_MODEL.md](docs/SECURITY_MODEL.md).

## Supported versions

Only the latest release on the stable channel receives fixes.
