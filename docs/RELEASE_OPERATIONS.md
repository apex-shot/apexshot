# Release operations: support, promotion, and rollback

The operational half of shipping a stable desktop release: where breakage is
reported, what has to be configured before a tag is pushed, and what can still
be undone afterwards. Packaging mechanics live in
[AUR_PUBLISHING.md](AUR_PUBLISHING.md); the pricing and licence position lives
in [COMMERCIAL_TRANSITION.md](COMMERCIAL_TRANSITION.md).

## Support path

Public support is the GitHub issue tracker
([README](../README.md) links the new-issue chooser). It covers install
failures, capture and recording faults, editor and export faults, and update
problems. Security reports go through [SECURITY.md](../SECURITY.md) instead, not
a public issue.

Ask a reporter for the details a diagnosis actually needs: the version or tag,
distribution and version, desktop and compositor with the session type
(Wayland or X11), install method, whether Studio is activated, and the relevant
log excerpt. A green CI run is not evidence about a user's desktop, so treat
their report as the primary artifact.

Purchase and refund questions belong to the payment provider, not this
repository. The desktop app can validate a signed entitlement and nothing else:
it cannot look up an order, reissue a token, or revoke one. Never ask a
reporter to paste an entitlement into an issue; it is a credential. Ask for the
Studio status line in Settings instead.

## Before tagging

1. **Fix the version.** `Cargo.toml` and the tag agree, the tag is
   `vMAJOR.MINOR.PATCH`, and the release marker advances beyond the previous
   stable marker. A marker that does not advance fails promotion.
2. **Configure the Studio build variables.** `APEXSHOT_STUDIO_VERIFY_KEY` (43
   characters of unpadded base64url for 32 bytes) and
   `APEXSHOT_STUDIO_RELEASE_TIMESTAMP` (positive Unix seconds, pinned for this
   build) must resolve for the tag run. Repository variables are directly
   readable with repository admin; organization-level values need `admin:org`
   scope, so prefer repository level when you need to prove the effective
   configuration. Validate without a build:

   ```sh
   python3 scripts/release_gate.py validate-config "$KEY" "$TS"
   ```

3. **Match the signer by fingerprint, not by copying keys.** The fingerprint is
   the SHA-256 of the 32 key bytes, so it can be compared without printing the
   key itself:

   ```sh
   python3 -c 'import sys; sys.path.insert(0, "scripts"); from release_gate import public_key_fingerprint; print(public_key_fingerprint(sys.argv[1]))' "$KEY"
   ```

   Compare that hex string against the backend signer's fingerprint. The signing
   seed never appears in this repository, a package, a log, or a workflow
   variable.
4. **Confirm the `stable-release` environment exists.** Promotion runs in that
   environment. GitHub creates a missing environment on first use with no
   protection rules, so an absent environment looks like a working approval step
   while being none. It must carry a required reviewer and a tag-only deployment
   policy. Verify:

   ```sh
   gh api repos/apex-shot/apexshot/environments/stable-release \
     --jq '{rules: [.protection_rules[].type],
            reviewers: [.protection_rules[].reviewers[]?.reviewer.login],
            tags: .deployment_branch_policy}'
   ```

   Self-review stays allowed: the maintainer is the only reviewer, and blocking
   self-review would make promotion impossible. The workflow's actor check and
   exact confirmation string remain required anyway.
5. **AUR, only if publishing there.** `AUR_SSH_PRIVATE_KEY` and a pinned
   `AUR_SSH_KNOWN_HOSTS` entry must be configured. Never discover the host key
   with `ssh-keyscan`. A missing AUR credential fails that job after the GitHub
   release is already published; it does not roll anything back.
6. **Reconcile the public copy.** The store page, checkout terms, and
   [RELEASE_NOTES_CANDIDATE.md](RELEASE_NOTES_CANDIDATE.md) must state the same
   price, update window, and refund window before the draft goes public.

## Tagging and promotion

A tag run builds the Debian, Fedora, openSUSE, and Arch packages and stages a
**draft** release with checksums. The tag-only jobs never run on `main`, so a
green `main` run says nothing about the release path; only a tag run exercises
it.

1. Push the tag. The run validates the Studio configuration, then builds.
2. Review the draft: every expected asset present, `SHA256SUMS` covering all of
   them, and no unexpected extras. Paste the notes preface above the generated
   changelog.
3. Publish by dispatching **Promote stable release** from `main`, entering the
   stable tag and typing `PUBLISH <tag>`. `scripts/release_gate.py
   authorize-promotion` requires the configured maintainer, `refs/heads/main`,
   a stable tag, and that exact confirmation; the `stable-release` environment
   approval is a second layer on top.
4. The AUR job follows a successful publication.
5. **Check the announcement posted.** A release published with the default
   `GITHUB_TOKEN` does not trigger other workflows, so the
   `release: published` webhook can be skipped (see
   [DEAD_CODE_AND_QUALITY_AUDIT.md](DEAD_CODE_AND_QUALITY_AUDIT.md), BG-27).
   Post the announcement manually when the channel stays quiet.

## Rollback

Cost rises sharply with each window. Decide which one you are in before acting.

| Window | What still works | What to do |
| --- | --- | --- |
| Draft, not promoted | Nothing is public | Delete the draft and the tag, fix, re-tag. Nothing has been announced. |
| Published, asset or feature broken | Assets are public; the tag workflow refuses to overwrite a published release | Do not delete it. Publish a corrective patch release, and edit the release body with a known-issues note. If installs must stop, marking the release `--prerelease` moves the "latest" pointer back to the previous release. |
| Published, AUR recipe wrong | The AUR package serves the bad recipe | Revert the recipe commit in the AUR repository; the GitHub assets are unaffected. |
| Paid path broken with sales open | Purchases continue | Stop sales in the provider first, then fix forward. Already-issued entitlements keep validating offline. |
| Cloud or hosting fault | Desktop is unaffected | Handled in the apexshot-cloud repository, not here. |

Two things a rollback cannot do. It cannot recall an installed package: the GPL
grant and offline entitlement both survive, so say "fixed in the next build",
never "we removed the affected version". And it cannot revoke an entitlement
already stored on a disconnected device, which is why refund copy promises a
server-side stop, not remote disabling.

Record the run URL, the environment output above, and the asset checksums with
the release so a later audit can tell which revision produced what.

## Out of scope

Flatpak and Flathub submission are separate and human-led; the local manifest
is not the submission. Backend deployment, product configuration, and refund
handling live in the apexshot-cloud repository.
