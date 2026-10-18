# Current commercial-transition decision

## Product and licence

ApexShot remains one maintained GPL-3.0-or-later desktop application. The
published free workflows stay free and maintained, including capture, image
annotation and editing, OCR/QR, recording and audio, released video editing,
legacy auto/manual zoom and cursor controls, existing backgrounds, and clean
exports that do not use a new Studio capability. Free use stays local-first:
no account, subscription, network connection, recording interruption, or
watermark is required.

The official desktop build offers optional **ApexShot Studio for US$39 once**.
It adds Motion video export and its animated titles/timing tools, offline
neural upscaling for small captures, non-default static presentation
transforms, and the genuinely new advanced zoom-camera controls. Studio
includes 12 months of updates; a build whose immutable release
marker is covered by the entitlement continues to work offline after that
period. It is not a promise of lifetime future updates or hosting.

GPL rights remain unchanged. Payment does not restrict commercial use of the
GPL program, source access, modification, or redistribution. There is no
separate paid application, personal-use-only licence, or invasive DRM scheme.
Cloud is a separate optional service at US$5 per month; a Cloud subscription
does not grant Studio, and Studio does not grant paid hosting.

## Transition safeguards

The last stable desktop release verified for this decision is v0.2.35,
published 2026-09-06 at commit
`1f49b411a86c5cabdfe3afc7e22d33833b9c7d49`. Its old zoom model and controls
remain free; Studio gates only the new instant flag and non-default advanced
camera settings on visible, applicable zooms. The old Classic Animation switch,
per-clip easing, ease duration (including imported zero-duration values), and
default spring behavior are not paid capabilities.

New Studio controls are labelled before use. Local previews and editable
projects remain available without activation. Only final rendered outputs that
use a Studio capability require an eligible entitlement. Missing official
verification configuration blocks those outputs but does not affect free
capture, editing, recording, project access, or clean exports.

The existing GPL grant cannot be withdrawn from copies already released. Before
sales begin, review prior public statements about upcoming features; if a
specific Studio capability was explicitly promised as free, provide an
appropriate transition entitlement rather than silently changing that promise.
This repository change does not inspect or revise the separate website or
payment service.

The proposed refund window is 14 days on the first Studio purchase. Publish it
only with consistent checkout terms. An offline entitlement has no live
revocation channel: a refund cannot immediately disable a token already stored
on a disconnected device, and a covered build remains usable. Do not promise
instant revocation.

## Release readiness

Official releases now build packages into a complete, checksummed draft and
require an authorized manual stable-promotion action before publication. The
public verification key and pinned release timestamp must be configured as
GitHub Actions repository variables before a tag build; missing values stop
release preparation. The signing seed must remain outside this desktop
repository. The activation UI and token verifier do not provide purchase
processing, account recovery, or token issuance; those require a separately
authorized service and secure operational configuration.
