# AUR Publishing

The Arch package is prepared and published to AUR only after a maintainer
explicitly promotes a complete stable draft release. Pushing a tag only builds
the packages and stages a draft; it does not publish the stable release or
change AUR.

## Required configuration

Configure these GitHub Actions repository variables before preparing an
official release:

- `APEXSHOT_STUDIO_VERIFY_KEY`: the canonical unpadded base64url Ed25519
  verification key (32 decoded bytes).
- `APEXSHOT_STUDIO_RELEASE_TIMESTAMP`: the positive Unix UTC seconds marker
  pinned into this release's binaries.

The tag workflow fails before package builds when either variable is missing
or malformed. Never put an entitlement signing seed in desktop source,
packages, workflow variables, or this repository. The AUR recipe contains only
the public verification key and release timestamp for its specific stable
version; source builds without those values keep Studio output unavailable.

The workflow also needs `AUR_SSH_PRIVATE_KEY` and a pinned
`AUR_SSH_KNOWN_HOSTS` entry for `aur.archlinux.org`. It refuses to publish to
AUR if either is missing; it never trusts an unverified host key discovered
with `ssh-keyscan`. A stable release can already be published after explicit
approval, so this failure does not roll back the GitHub release assets.

## Release flow

1. A version tag runs CI, validates the official Studio build configuration,
   builds Debian/GNOME, openSUSE, Fedora, and Arch artifacts, then verifies the
   complete package set and checksums before creating or refreshing a **draft**
   release.
2. Review the draft assets and notes. To publish, dispatch **Promote stable
   release** from `main`, enter the stable `vMAJOR.MINOR.PATCH` tag, and type
   `PUBLISH <tag>` exactly.
3. The workflow permits only the configured maintainer, verifies the tag is
   reachable from `main`, checks every staged asset and checksum, and requires
   the release marker to advance beyond the previous stable marker. Configure
   the `stable-release` GitHub Environment with required reviewers as an
   additional approval layer; the workflow's actor and exact-confirmation
   checks remain required even when that environment exists.
4. Only after the draft is published does the dependent AUR job update
   `PKGBUILD` and `.SRCINFO`, verify the source archive, and push the recipe.

The asset gate includes every RPM produced by both RPM builds, including debug
variants, along with source RPMs, the Debian package, GNOME extension archive,
Arch package, and `SHA256SUMS`. A missing or duplicate asset, a checksum
mismatch, or a stale Studio marker prevents promotion.

## Local preparation helper

`scripts/aur-prepare.sh <tag> <public-key> <release-timestamp>` prepares a
versioned recipe when the public build configuration is available. Without
the optional values, it prepares a source recipe that compiles without Studio
unlock configuration. This helper does not publish anything.
