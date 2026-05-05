# AUR Publishing

This repository publishes the `apexshot` AUR package from tagged releases.

## One-Time AUR Setup

1. Create or log in to an Arch User Repository account.
2. Add an SSH public key to the AUR account.
3. Create a matching private key for GitHub Actions and store it as:

   `AUR_SSH_PRIVATE_KEY`

4. Optionally pin the AUR host key as:

   `AUR_SSH_KNOWN_HOSTS`

   You can generate it with:

   ```bash
   ssh-keyscan -t rsa,ed25519 aur.archlinux.org
   ```

If `AUR_SSH_PRIVATE_KEY` is not configured, release CI builds the Arch package
but skips the AUR publish step.

## Release Flow

Tag releases as usual:

```bash
git tag v0.2.27
git push origin v0.2.27
```

The `Publish AUR package` workflow job will:

1. Update `packaging/arch/PKGBUILD` for the tag version.
2. Download the GitHub source archive and compute its `sha256sum`.
3. Regenerate `packaging/arch/.SRCINFO`.
4. Verify the source archive with `makepkg --verifysource --nobuild`.
5. Push `PKGBUILD`, `.SRCINFO`, and `apexshot.install` to:

   `ssh://aur@aur.archlinux.org/apexshot.git`

## Local Dry Run

Run this on Arch Linux or in an Arch container:

```bash
scripts/aur-prepare.sh v0.2.27
cd packaging/arch
makepkg --verifysource --nobuild
makepkg --printsrcinfo > .SRCINFO
```

Then inspect:

```bash
git diff -- packaging/arch/PKGBUILD packaging/arch/.SRCINFO
```

## Manual Publish

Use this only if CI publishing fails:

```bash
scripts/aur-prepare.sh v0.2.27
git clone ssh://aur@aur.archlinux.org/apexshot.git /tmp/apexshot-aur
cp packaging/arch/PKGBUILD packaging/arch/.SRCINFO packaging/arch/apexshot.install /tmp/apexshot-aur/
cd /tmp/apexshot-aur
git add PKGBUILD .SRCINFO apexshot.install
git commit -m "Update to 0.2.27"
git push
```

## Notes

- The AUR package builds from the GitHub source archive, not from the prebuilt
  release package.
- Do not publish `*.pkg.tar.zst` files to AUR.
- Keep `.SRCINFO` synchronized with `PKGBUILD`; AUR reads package metadata from
  `.SRCINFO`.
