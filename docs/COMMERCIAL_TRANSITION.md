# ApexShot commercial-transition decision note

## Decision in principle

Moving ApexShot to a sustainable commercial model is reasonable. Maintaining a
desktop product across distributions, desktop environments, capture APIs, and
eventually Windows is real work. Burnout is a business signal: if development
cannot sustain its maintainer, users do not have a sustainable product either.

The recommended route is **not** to try to erase the open-source history.
Keep the current GPL release available as the final Community / 1.x release,
then release ApexShot 2.0 and later under a proprietary licence once the legal
review is complete. The new application has a free Personal tier and paid Pro
and Team tiers; it is not an ongoing open-source Community Edition.

Use “closed source” or “proprietary”, rather than “close source”, in public
communication.

## Product plan

1. Preserve the last GPL release as ApexShot Community / 1.x. It can receive
   critical fixes, but no promise needs to be made to continue major feature
   development indefinitely.
2. Release ApexShot 2.0 as proprietary software with a Free Personal licence.
   Keep this tier local-first, require no payment, and let people evaluate the
   product without an artificial watermark.
3. Sell ApexShot Pro to freelancers, creators, and businesses. It includes a
   commercial-use licence as well as clear workflow upgrades; commercial rights
   must not be the only reason to pay because some users will ignore a licence
   restriction.
4. Validate demand before taking on Windows: create a Pro landing page,
   collect emails, and sell discounted founding licences for a Linux Pro beta.
5. Add Windows after paid demand is clear. Windows increases reach, but also
   adds installer and code-signing work, DPI and multi-monitor testing, audio
   device variations, capture permissions, and support obligations.

## Freemium pricing hypothesis

The model is proprietary freemium: users may install ApexShot 2.0 and later
without paying for genuinely personal use, while professional use and premium
workflows pay for the product's continued development.

| Tier | Suggested price | Licence | Product boundary |
| --- | --- | --- | --- |
| Free Personal | US$0 | Personal, hobby, and student use only | Local screenshots, annotation, OCR/QR, and basic local recording (for example, up to 720p/30fps). No included cloud storage. |
| Pro | US$29 once with optional paid updates, or US$4–6/month | Commercial use for one person | 1080p/60fps, unlimited recordings, pause/resume, skip countdown, polished recording editor, cursor/motion effects, premium exports, and priority support. |
| Team | Launch only after demand exists | Commercial use for managed teams | Shared cloud storage, central billing, user management, branded links, and priority support. |

Choose one Pro billing model before launch. A one-time licence plus optional
updates better suits a local desktop app. A subscription is more suitable when
it includes ongoing costs such as cloud storage, bandwidth, transcription, or
team administration. Do not promise “free cloud storage forever” until paid
revenue reliably covers storage and bandwidth.

ScreenRec demonstrates the general packaging approach: its free tier is for
personal use, while paid tiers allow business use and add recording quality,
cloud, and support benefits. ApexShot should use the idea, not copy its exact
limits or prices.

The 1,314 total downloads are encouraging but are not proof of a business.
At a US$25 Pro price and a speculative 1–5% conversion, they correspond to
roughly US$330–1,640 gross revenue before payment fees, refunds, and tax.
Measure active users, email signups, and founding-licence purchases instead of
basing the Windows investment on lifetime downloads.

### Define personal use clearly

The EULA and pricing page should say that the Free Personal licence excludes
use for an employer, a client, freelance/contract work, a business's internal
operations, sales, marketing, support, or training. Decide and state a clear
rule for monetised content creation (such as YouTube), education, charities,
and open-source maintainers. Avoid vague wording such as “not for commercial
use” without examples.

## Does ApexShot need a lawyer?

Not necessarily as a long-term retainer. But a **one-time review by a software
licensing lawyer is strongly recommended before distributing a proprietary
release**. It is cheap compared with discovering after launch that the binary
must be open-sourced, a contributor has rights you did not obtain, or the
sales/telemetry terms are incomplete. This document is a product note, not
legal advice.

The review should answer these exact questions:

1. **Who owns every part of ApexShot?** A new proprietary licence can be issued
   only by the relevant copyright holder(s). Confirm ownership of employee,
   contractor, contributor, and generated-code contributions, and that no
   contributor agreement prevents relicensing. The Git history should be
   reviewed as evidence, not assumed to settle ownership.
2. **What is in the distributed application?** Audit Rust crates, C++/Qt code,
   copied snippets, fonts, cursors, icons, browser and GNOME-extension code,
   build tooling, and bundled binaries. Record each licence and required notice
   in a software bill of materials (SBOM).
3. **Does any GPL code remain in Pro?** A proprietary combined program cannot
   include GPL-covered code from someone else without a separate commercial
   permission from that copyright holder. Merely changing this repository's
   `LICENSE` file does not solve that.
4. **Can the remaining dependency obligations be met?** Permissive licences
   (such as MIT, BSD, or Apache-2.0) commonly permit proprietary distribution
   when their notices and conditions are followed; LGPL and other copyleft
   dependencies have more conditional requirements. Get advice on the actual
   architecture and licences rather than relying on labels alone.
5. **Are the commercial documents ready?** Prepare the EULA, refund policy,
   privacy policy, support terms, and telemetry/cloud disclosures. If payments
   or cloud storage are involved, use a merchant of record or obtain advice on
   taxes, VAT/sales tax, and data-protection duties in the markets served.
6. **Is the brand protected?** Decide whether to register the ApexShot name and
   logo as trademarks in the markets that matter. This is separate from the
   source-code licence and helps distinguish the paid product from GPL forks.

### Why sole authorship matters

If you wrote the original ApexShot code yourself, you normally still own its
copyright even though you released it under GPL. GPL gives users permissions to
use, copy, modify, and distribute the GPL versions; it does **not** transfer
your copyright to them. As the owner, you may offer your own future code under
a separate proprietary/commercial licence as well.

The preliminary Git history check lists only identities associated with the
project maintainer plus an automation bot. That is encouraging, but it is not a
complete ownership audit: confirm that no employer, contractor, friend, or
external contributor wrote code that remains in the product, and that no code
was copied from a project under GPL or another restrictive licence.

### Dependency and asset audit checklist

Before a closed-source Pro build, make one inventory with the item, version,
licence, copyright holder, source URL, how it is used, and required notices for
each of the following:

- Rust crates in `Cargo.toml` and `Cargo.lock`, including optional and
  build-time dependencies.
- C++/Qt libraries, system libraries, native capture helpers, codecs, and any
  bundled executables.
- Fonts, cursor files, icons, logos, illustrations, screenshots, and media.
- Code snippets copied from Stack Overflow, GitHub, documentation, tutorials,
  or other applications.
- The GNOME extension, browser extension, installer scripts, package metadata,
  translations, and cloud/backend code shipped with or required by Pro.

Classify each item before release:

| Result | What to do |
| --- | --- |
| Your original work | Include it under the new commercial licence. |
| MIT/BSD/Apache-2.0 or similar permissive component | Usually usable in proprietary software, but preserve its required licence and copyright notices. |
| LGPL or other conditional copyleft component | Obtain advice on the actual linking/distribution arrangement and comply with its conditions. |
| GPL/AGPL component owned by someone else | Remove, replace, keep it in an open-source separate program where appropriate, or obtain a commercial licence/permission. Do not include it in a proprietary combined app without resolving the licence. |
| Unknown origin or licence | Do not ship it in Pro until its origin and permission are verified. |

Keep the completed inventory (an SBOM) with release records. It makes future
updates, due diligence, and any lawyer's review substantially faster.

## What the existing GPL release means

ApexShot is currently licensed as GPL-3.0-or-later. That grant for already
released copies is irrevocable: users may keep, share, and fork the versions
that were released under GPL. Removing the GitHub repository, download links,
or source files cannot turn those historical releases into proprietary code.

That does **not** automatically prevent a future proprietary release. If the
right holders control all relevant original copyright, they can offer their own
code under more than one licence (dual licensing), including a commercial
licence for a future release. They cannot, however, unilaterally relicense
third-party GPL contributions or third-party GPL components included in the
product. The GNU GPL FAQ discusses both the copyright-holder requirement for
licensing exceptions and the requirement for a GPL-compatible licence when
combining GPL code into a larger program.

## Before announcing the change

- Tag and archive the final Community GPL release; retain its licence and
  notices.
- Obtain the licensing/SBOM review and remove, replace, separately license, or
  continue open-sourcing any component that blocks a proprietary release.
- Create the Pro EULA, privacy policy, refund policy, licence-activation
  policy, and support boundary.
- Test willingness to pay with a landing page and founding licences before
  committing to the Windows port.
- Tell existing users plainly: “ApexShot needs a sustainable model. Community
  1.x stays available under GPL. ApexShot 2.0 is closed source, free for
  personal use, and paid for professional use; Pro development funds the
  product.”

## Reference material

- GNU GPL FAQ: <https://www.gnu.org/licenses/gpl-faq.en.html>
- GNU GPL licence compatibility and relicensing: <https://www.gnu.org/licenses/license-compatibility.en.html>
- CleanShot pricing: <https://cleanshot.com/pricing>
- Shottr purchase options: <https://shottr.cc/purchase.html>
