# Security policy

SoundPush carries microphone audio between devices, so security reports are taken seriously.

## Reporting a vulnerability

Please **do not open a public issue**. Use GitHub's private vulnerability reporting
(Security → Report a vulnerability) on this repository. Include:

- affected version(s) and platform,
- steps to reproduce or a proof of concept,
- the impact you observed.

We aim to acknowledge reports within 3 working days and to ship a fix for critical
issues within 30 days. After a fix is released we publish an advisory and credit the
reporter (unless you prefer to stay anonymous).

## Scope

In scope: pairing and authentication, transport encryption, permission enforcement,
the discovery protocol, packet parsing, the Windows virtual audio driver, and the
desktop/mobile apps' handling of untrusted input.

The design is documented in [`docs/security/`](docs/security/).
