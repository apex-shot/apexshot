# Release notes: next stable desktop release

Text to place above the generated changelog in the draft release body. The tag
workflow creates the draft with `--generate-notes`, which lists merged work but
says nothing about pricing, gating, or limits. Paste this preface into the
draft, adjust the version line, and delete this file once the release ships so
the next cycle starts from a clean page.

Pricing and refund wording here must stay identical to
[COMMERCIAL_TRANSITION.md](COMMERCIAL_TRANSITION.md) and to the in-app upgrade
card (`src/studio_upsell.rs`). If the checkout terms change, change all three
in the same commit.

---

## ApexShot vNEXT

ApexShot stays one free, GPL-3.0-or-later desktop app. Capture, annotation,
OCR and QR, recording and audio, the video editor, the legacy auto and manual
zoom, cursor controls, the existing backgrounds, and clean exports that use no
new Studio capability remain free and need no account, subscription, or network
connection.

### ApexShot Studio for US$39 once

Studio adds Motion video export with its animated titles and timing tools,
non-default static presentation transforms, and the advanced zoom-camera
controls. It includes 12 months of updates: the builds you receive during that
window keep working offline afterwards. It is not a lifetime-updates or hosting
promise, and it does not include Cloud.

Studio controls are labelled before you use them. Previewing, editing, and
saving projects never needs activation; only a final render that actually uses
a Studio capability does. Free exports from a Studio-configured build behave
exactly as they did before.

Activation is offline. Paste the signed entitlement you receive after purchase,
and it keeps validating on that device without a network connection. An
entitlement already stored on a disconnected device cannot be revoked remotely,
so a refund stops server-side recovery rather than disabling a copy you already
hold.

### Refunds

14 days on your first Studio purchase. Cloud is separate at US$5 per month and
is not affected by a Studio refund, and cancelling Cloud does not revoke Studio.

### Known limits

- Fedora does not offer recording. ApexShot refuses it there with a notice
  instead of starting a recording that cannot finish.
- Cloud is optional and separate from Studio.
- Package availability differs per distribution. Installers and the Debian,
  RPM, Arch, and NixOS instructions in the README cover the supported routes.

### Checking the download

The draft carries a `SHA256SUMS` file covering every published asset. Verify it
before installing, especially from a mirror or a third-party download page.

---

## Before pasting this

- Confirm the store is serving the same price, update window, and refund terms.
  If it is not, hold the draft; do not publish notes that disagree with checkout.
- Replace `vNEXT` with the tagged version.
- Delete any section that does not apply to the shipped asset set, such as the
  Fedora limit if that distribution is not in the release.
