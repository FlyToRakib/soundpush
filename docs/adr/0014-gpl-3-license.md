# ADR-0014: GPL-3.0-or-later license

- Status: accepted
- Date: 2026-09-14
- Plan: §36.1, §36.2, decision log ADR-0014

## Context

SoundPush should stay free for users, including when others redistribute it (store builds, forks, bundles).
Planned dependencies are MIT, Apache-2.0, BSD, ISC, Zlib and MPL-2.0 (libopus BSD, RNNoise BSD, SysVAD sample MIT,
ZXing Apache-2.0). The permissive alternative (MIT/Apache-2.0) would allow closed-source forks.

## Decision

- The whole repository is licensed **GPL-3.0-or-later**. `LICENSE` contains the full GPL-3.0 text; `Cargo.toml`,
  `package.json` and the bundles declare `GPL-3.0-or-later`.
- Contributions are accepted under the same license with a DCO sign-off (`git commit -s`).
- Dependencies must be compatible: `deny.toml` allows MIT, Apache-2.0, BSD, ISC, Zlib, MPL-2.0, LGPL, GPL-3.0 and
  similar, and bans GPL-2.0-only and proprietary licences. CI runs `cargo deny check`.
- Apps show license information (Settings → About); an attribution screen generated from dependency metadata
  follows.

## Consequences

- Anyone distributing modified builds must publish their source under the GPL.
- Apache-2.0 dependencies are fine with GPL-3.0 (not with GPL-2.0-only), one reason for "3.0-or-later".
- Google Play distribution is allowed; the source must stay available for every published build.
- Bundled third-party components keep their own terms (VB-CABLE is VB-Audio donationware, installed from its
  official package, not relicensed).
