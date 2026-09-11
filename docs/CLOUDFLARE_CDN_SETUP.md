# Cloudflare CDN + branded installer setup

Goal: replace

```
curl -fsSL https://raw.githubusercontent.com/apex-shot/apexshot/main/scripts/install.sh | bash
```

with

```
curl -fsSL https://apexshot.org/install | sh
```

…and move release artifact downloads off `github.com/releases` onto your own
Cloudflare edge.

**Prerequisite:** `apexshot.org` is already on Cloudflare (it is). R2 custom
domains and Worker routes both require the zone to be on Cloudflare.

## What moves and what does not

| Thing | Current source | After |
|---|---|---|
| Installer scripts | `raw.githubusercontent.com` | `apexshot.org/install` (Worker) |
| `.deb` / `.rpm` / `.pkg.tar.zst` / GNOME ext zip | `github.com/.../releases/download` | `dl.apexshot.org` (R2) |
| System deps (`libgtk-4-1`, `gstreamer1.0-*`, `qt5-base`, …) | distro mirrors | **unchanged** |

System dependencies must stay on distro mirrors — that is where they are signed
and updated. Only ApexShot's own artifacts move.

## Architecture

```
apexshot.org/install            -> Worker  -> R2 "scripts/install.sh", else GitHub raw
apexshot.org/update             -> Worker  -> R2 "scripts/update.sh",  else GitHub raw
apexshot.org/install/{ubuntu,arch,fedora}
                                -> Worker  -> per-distro installer

dl.apexshot.org/latest          -> R2 object, plain text: "v0.2.35"
dl.apexshot.org/v0.2.35/…       -> R2 objects: .deb, .rpm, .pkg.tar.zst, ext zip, SHA256SUMS
```

Artifacts are served from the **R2 custom domain**, not through the zone cache
or the Worker. R2 egress is free and this keeps you clear of Cloudflare's
self-serve terms on large non-HTML payloads through the zone CDN.

---

# Step 1 — Enable R2 and create the bucket

1. Cloudflare dashboard → **R2** → **Enable R2**. A payment card is required on
   file even to stay inside the free tier (10 GB storage, $0 egress).
2. **Create bucket** → name: `apexshot-releases` → location: Automatic.

The bucket name must match `bucket_name` in `scripts/cloudflare-wrangler.toml`.

# Step 2 — Create an R2 API token

1. R2 → **Manage API Tokens** → **Create API Token**.
2. Permissions: **Object Read & Write**.
3. Scope it to the `apexshot-releases` bucket only.
4. Save the values — the secret is shown once:
   - Access Key ID
   - Secret Access Key
   - your **Account ID** (also visible in the dashboard URL:
     `dash.cloudflare.com/<ACCOUNT_ID>/…`)

# Step 3 — Add the GitHub Actions secrets

Repo → **Settings → Secrets and variables → Actions → New repository secret**:

| Secret | Value |
|---|---|
| `R2_ACCOUNT_ID` | your account ID |
| `R2_ACCESS_KEY_ID` | access key ID from step 2 |
| `R2_SECRET_ACCESS_KEY` | secret access key from step 2 |

# Step 4 — Bind the R2 custom domain

1. R2 → `apexshot-releases` → **Settings** → **Custom Domains** →
   **Connect Domain**.
2. Enter `dl.apexshot.org`. Cloudflare creates the DNS record automatically.
3. Make sure there is **no pre-existing A/CNAME record** for
   `dl.apexshot.org` — one will block the binding. Delete it first if so.
4. Wait for the cert to issue (usually < 2 minutes).

Add the lifecycle rule while you are here:

- R2 → bucket → **Settings** → **Object Lifecycle Rules** → **Add rule**
- Name: `expire-old-releases`
- Prefix: `v`
- Action: **Delete objects** after **180** days

The `v` prefix only matches versioned paths (`v0.2.35/…`), so `latest` and
`scripts/` are never expired. Without this the 10 GB free tier eventually
fills; with it you stay at $0 indefinitely.

# Step 5 — Deploy the Worker

From the repository root:

```bash
cd /path/to/apexshot

# One-time auth (opens a browser), or export CLOUDFLARE_API_TOKEN=… instead.
npx wrangler login

# Deploy
npx wrangler deploy --config scripts/cloudflare-wrangler.toml
```

Wrangler will register the two routes from the config:
`apexshot.org/install*` and `apexshot.org/update*`.

Because these are **routes** (not a zone-wide Worker), the rest of
apexshot.org — homepage, docs, everything — continues to serve from its
existing origin untouched.

CI-only alternative: create a Cloudflare API token with the
**Edit Workers** permission and add it as the `CLOUDFLARE_API_TOKEN` repo
secret, then add a `wrangler-action` deploy job. Not required.

> If you prefer the files under `infra/cloudflare/` instead of `scripts/`:
> `mkdir -p infra/cloudflare && git mv scripts/cloudflare-worker.js infra/cloudflare/worker.js && git mv scripts/cloudflare-wrangler.toml infra/cloudflare/wrangler.toml`
> then change `main` in the toml to `worker.js`.

# Step 6 — Verify

```bash
# Should print a shebang and shell code, and x-apexshot-source: github (until step 7)
curl -fsSL https://apexshot.org/install | head -n 3

# Confirm the routing headers
curl -fsSLI https://apexshot.org/install | grep -iE 'x-apexshot-source|content-type'

# Per-distro entrypoints
curl -fsSL https://apexshot.org/install/ubuntu | head -n 3
curl -fsSL https://apexshot.org/install/arch   | head -n 3
curl -fsSL https://apexshot.org/install/fedora | head -n 3

# Unknown path -> 404 (and NOT a script)
curl -fsSI https://apexshot.org/install/nope | head -n 1

# The rest of the site must be unaffected
curl -fsSI https://apexshot.org/ | head -n 1
```

# Step 7 — Mirror artifacts and scripts from CI

The existing `checksums` job in `.github/workflows/release.yml` already
downloads **every** release asset (`.deb`, `.rpm`, Arch package, GNOME
extension zip) and generates `SHA256SUMS`. Append this step to that job, right
after the `gh release upload … SHA256SUMS` line:

```yaml
      - name: Mirror release assets to Cloudflare R2
        env:
          AWS_ACCESS_KEY_ID: ${{ secrets.R2_ACCESS_KEY_ID }}
          AWS_SECRET_ACCESS_KEY: ${{ secrets.R2_SECRET_ACCESS_KEY }}
          R2_ENDPOINT: https://${{ secrets.R2_ACCOUNT_ID }}.r2.cloudflarestorage.com
        run: |
          set -euo pipefail
          V="${GITHUB_REF_NAME}"
          BUCKET="s3://apexshot-releases"

          # Version-pinned, immutable artifacts.
          aws s3 cp --recursive /tmp/apexshot-assets "${BUCKET}/${V}/" \
            --endpoint-url "$R2_ENDPOINT"

          # Mutable "latest" pointer the installers read.
          # max-age=60 so a rollback propagates fast.
          printf '%s' "$V" > /tmp/apexshot-latest
          aws s3 cp /tmp/apexshot-latest "${BUCKET}/latest" \
            --endpoint-url "$R2_ENDPOINT" \
            --content-type text/plain \
            --cache-control 'max-age=60'

          # Mirror the installer scripts so the entrypoint does not depend on
          # GitHub being reachable. Excludes the dev-only helper scripts.
          aws s3 sync scripts/ "${BUCKET}/scripts/" \
            --endpoint-url "$R2_ENDPOINT" \
            --exclude '*' \
            --include 'install.sh' --include 'update.sh' \
            --include '*-install.sh' --include '*-update.sh' \
            --content-type 'text/x-shellscript; charset=utf-8' \
            --cache-control 'max-age=300'
```

`aws` CLI is preinstalled on `ubuntu-latest`, so no extra setup step is needed.

Verify after the next tag:

```bash
curl -fsSL https://dl.apexshot.org/latest
curl -fsSL https://dl.apexshot.org/v0.2.35/SHA256SUMS | head
curl -fsSLI https://apexshot.org/install | grep -i x-apexshot-source   # now "r2"
```

# Step 8 — Point the installers at the CDN

This is the code change that delivers the actual speed win. Until it lands,
users still download packages from GitHub even though the entrypoint is
branded.

Replace the release-page HTML scraping in each installer:

| File | Functions to replace |
|---|---|
| `scripts/ubuntu-install.sh` | `latest_release_tag`, `resolve_latest_gnome_extension_url`, `download_deb` |
| `scripts/arch-install.sh` | `latest_release_tag`, `resolve_latest_gnome_extension_url`, `fetch_version`, `install_from_release` |
| `scripts/fedora-install.sh` | `fetch_version`, `resolve_rpm_url` |
| `scripts/ubuntu-update.sh`, `arch-update.sh`, `fedora-update.sh` | same release-resolution helpers |
| `scripts/install.sh`, `scripts/update.sh` | the four hardcoded `raw.githubusercontent.com` handoff URLs |
| all four installers | stale URL strings in `summary()` |

Target pattern (`latest` + `SHA256SUMS` instead of HTML scraping):

```sh
CDN="https://dl.apexshot.org"

fetch_version() {
    VERSION="$(curl -fsSL "${CDN}/latest")"
    # fall back to GitHub if the CDN pointer is missing
    [ -n "$VERSION" ] || VERSION="$(github_latest_tag)"
    ...
}

download_deb() {
    local sums sha name
    sums="$(curl -fsSL "${CDN}/${VERSION}/SHA256SUMS")"
    line="$(printf '%s\n' "$sums" | grep -E 'amd64\.deb$' | head -n1)"
    sha="${line%% *}"; name="${line##* }"
    download_file "${CDN}/${VERSION}/${name}" "$deb_file" "deb"
    printf '%s  %s\n' "$sha" "$deb_file" | sha256sum -c - \
      || { err "Checksum mismatch — refusing to install."; exit 1; }
}
```

`SHA256SUMS` doubles as the manifest, so switching to the CDN also **adds**
checksum verification that the installers do not have today.

Also worth updating so Arch/Fedora users get the CDN too:

- `packaging/arch/PKGBUILD` → `source=()` URL (currently `github.com/.../archive/`)
- `packaging/fedora/apexshot.spec` → `Source0`

# Step 9 — Make the piped command actually work under `sh`

**Do not advertise `| sh` until this is fixed.** All installers are bash-only:

- `install.sh` / `update.sh`: `${desktop,,}` (bash 4), `[[ ]]`, `BASH_SOURCE`
- `ubuntu-install.sh`: arrays — `spin=('⠋' '⠙' …)`, `deps=(…)`

On Debian/Ubuntu `/bin/sh` is **dash**, so `curl … | sh` fails immediately for
most users. Options:

1. Rewrite `install.sh` / `update.sh` as strict POSIX (use `tr` for
   lowercasing, `.` instead of `source`, `$0` instead of `BASH_SOURCE`) and
   `exec bash -c` the per-distro scripts. Cleanest — one URL, no extra hop.
2. Serve a tiny POSIX stub from the Worker that re-execs bash.

Option 1 is recommended. Also fix in the same pass: the
`exec bash -c "$(curl …)"` handoffs in `install.sh`, `update.sh`,
`ubuntu-install.sh`, and `arch-install.sh` **do not forward `"$@"`**, so
`… | sh -s -- --force` silently loses the flag.

# Troubleshooting

| Symptom | Cause |
|---|---|
| `curl https://apexshot.org/install` returns the website | Route not registered, or wrong zone. Re-run step 5 and check Workers → Routes. |
| `x-apexshot-source: github` forever | R2 script sync has not run, or the object key is not `scripts/<file>`. Check the bucket contents. |
| R2 custom domain stuck pending | A conflicting `dl` DNS record exists. Delete it, re-add. |
| `502 installer temporarily unavailable` | Both R2 and GitHub failed. Check Worker logs (`wrangler tail`). |
| Site homepage broke | The route pattern is too broad. It must stay `apexshot.org/install*`, not `apexshot.org/*`. |
| Wrangler deploy fails on routes | The API token lacks **Edit Workers** / the zone is on a different account than the Worker. |

## Cost

$0/month. Workers free tier (100k requests/day), R2 free tier (10 GB storage,
zero egress), and the domain you already own. The lifecycle rule keeps you
inside the free tier permanently.

## Rollback

Delete the two routes in Workers → Routes. The zone immediately reverts to its
previous origin behaviour and the old `raw.githubusercontent.com` URLs keep
working unchanged.
