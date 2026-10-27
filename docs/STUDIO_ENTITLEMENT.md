# Studio entitlement contract

This file defines the desktop entitlement format and its offline behaviour.
It is separate from Cloud subscriptions and does not impose a licence
restriction on GPL source builds.

## Signed token

The desktop accepts a UTF-8 JSON envelope with exactly these fields:

```json
{
  "key_id": "studio-v1",
  "payload": "<unpadded base64url of the original payload bytes>",
  "signature": "<unpadded base64url of the 64-byte Ed25519 signature>"
}
```

The decoded `payload` is the original UTF-8 JSON byte sequence. The signature
is verified strictly over those exact bytes; verification never reserializes
the JSON. The payload has exactly these fields:

```json
{
  "schema_version": 1,
  "license_id": "550e8400-e29b-41d4-a716-446655440000",
  "product": "apexshot-studio",
  "issued_at": 1798761600,
  "updates_until": 1830297600
}
```

`schema_version` is the integer `1`; `license_id` is a UUID string; `product`
is exactly `apexshot-studio`; and both times are positive Unix UTC seconds
stored as signed 64-bit integers. `updates_until` must be greater than or equal
to `issued_at`. Unknown fields, malformed base64url, a wrong key id/product,
an invalid UUID, or an invalid signature reject the token. The payload carries
no name, email, account, machine identifier, or Cloud tier.

The verifier uses `ed25519-dalek` 2.x. Production desktop builds contain only
the public verification key. The production signing seed never belongs in
this repository, desktop packages, logs, or release assets. Source tests use a
synthetic seed only in test-only code; it is not compiled into desktop builds.

## Build configuration and update eligibility

Official builds read these build-time environment variables:

- `APEXSHOT_STUDIO_VERIFY_KEY`: canonical unpadded base64url for exactly 32
  Ed25519 public-key bytes.
- `APEXSHOT_STUDIO_RELEASE_TIMESTAMP`: a positive `i64` Unix UTC seconds
  marker pinned for this official build.

Because the app reads these values with `option_env!` at compile time, a build
made without the pair ships with Studio configuration unavailable. The
repository's local build scripts (`reinstall-dev-deb.sh` and
`scripts/fedora-reinstall.sh`) resolve the pair through
`scripts/studio-build-env.sh`, checking the environment first, then the
untracked `.env`, then the repository variables via `gh`; they refuse to
compile without it. Set `APEXSHOT_ALLOW_NO_STUDIO=1` to explicitly opt out and
build without Studio configuration. The official workflows are unaffected.

The tag workflow validates both GitHub Actions repository variables before
building any release packages. One validated pair is passed to every package
build and recorded with the staged release. Promotion verifies that the draft
still matches the configured key fingerprint and marker and that the marker is
newer than the prior Studio release marker. It also rejects a changed
verification-key fingerprint until a token migration is designed, so replacing
the configured public key cannot silently invalidate existing licenses. Never
generate the marker from the desktop's runtime clock. Once compiled, a binary's
marker is immutable even if the repository variable changes later.

Studio output is eligible exactly when the official build's embedded release
timestamp is less than or equal to the token's `updates_until`. The app version
string and current wall clock do not affect eligibility. A covered build keeps
working indefinitely after the update period; a later build outside the period
does not unlock Studio output. Reinstalling a covered build restores access.

Update notices compare a verified license with the candidate release's
`STUDIO_RELEASE_METADATA.json` marker, and only when its schema and tag match
the release. GitHub's mutable publication date is not used as the build marker.
If that metadata is missing or invalid, the app says coverage is unknown and
continues to offer the update; a known uncovered release is also installable,
but the notice explains that Studio output will not work on it. Updates,
including security updates, are never blocked by this Studio warning.

If either build variable is missing or malformed, the Settings page reports
that Studio configuration is unavailable and Studio final output fails closed.
Free capture, editing, recording, project access, previews, and clean exports
remain unaffected. Local source builds with no official configuration still
work as the GPL program; this check is not a restriction on modifying or
redistributing the source.

## Local activation and recovery

Open **Settings → Studio**, paste the signed entitlement, and select **Activate**.
The token is verified locally and stored at
`${XDG_CONFIG_HOME:-~/.config}/apexshot/studio/entitlement.json` with private
directory/file permissions and atomic replacement. A malformed or invalid
replacement does not overwrite a previously valid token. The app reloads and
verifies the token from disk when checking access, so activation survives
restart and works offline.

Use the **Purchase or recover Studio** link to open `https://apexshot.org/studio`
in a browser. The desktop does not call a purchase, account, or recovery API;
the person providing the token must verify purchase ownership before issuing
one. The desktop repository contains no production signing key or issuance
service, so official sales and automated recovery are not ready until those
separate operations are configured.

When an export, copy, or upload would use a Studio capability without an
eligible token, the editor raises an in-window card instead of a desktop
notification. On an eligible build, a missing license offers an upgrade link
and inline activation; activating a covered license re-runs the blocked
export. An invalid saved license offers recovery and activation without a
purchase pitch. A build that cannot verify licenses links to the official
Studio availability and recovery page and explains that buying a license alone
cannot enable verification in that build. A token whose update period does not
cover the running build offers license management without a repurchase prompt.

Previewing Studio features and saving/opening editable projects remain free.
Only final clean rendering that actually uses a Studio capability requires an
eligible token. Hidden or inactive zoom clips and unused camera settings do not
trigger the gate. Legacy zoom easing, classic animation, shared rendering
improvements, normal image backgrounds, and default zoom-camera settings stay
free.

## Refunds and offline limits

The proposed policy is a 14-day refund window on the first Studio purchase and
must be stated consistently in the checkout terms before sale. This offline
token has no online revocation list or device binding. If a refund is processed
while a device is offline, the stored token cannot be immediately revoked
there, and a previously covered build remains usable. Do not promise immediate
refund revocation or use a Cloud cancellation to revoke a desktop entitlement.

For release staging and promotion steps, see [`AUR_PUBLISHING.md`](AUR_PUBLISHING.md).
