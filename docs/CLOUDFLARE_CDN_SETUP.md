# Cloudflare installer entrypoint and release mirror

One Worker (`apexshot-install`, in `scripts/cloudflare-worker.js`) and one R2
bucket (`apexshot-releases`) serve ApexShot's installers and release files
from Cloudflare's edge instead of GitHub.

```
apexshot.org/install, /update        -> Worker -> R2 "scripts/<file>", else GitHub raw
apexshot.org/install/{ubuntu,arch,fedora}, /update/{…}
                                     -> Worker -> per-distro script, same order
apexshot.org/download/windows        -> Worker -> 302 to dl.apexshot.org/<tag>/ApexShot-<version>-setup.exe,
                                                  else to the GitHub asset

dl.apexshot.org/latest               -> R2 object, plain text: the newest mirrored tag
dl.apexshot.org/<tag>/<asset>        -> R2 object: the mirrored release asset
```

Artifacts are served by the bucket's **custom domain**, not through the
Worker, so R2's free egress applies and large files never count against
Worker limits.

## How the mirror stays current

Nothing in CI uploads to R2, and nobody has to clean it up. The Worker has a
cron trigger (every 15 minutes) that:

1. Reads the latest GitHub release.
2. Copies each user-facing asset into `<tag>/<name>`: the Windows installer,
   the `.deb`, the RPMs, the Arch package, the GNOME extension zip,
   `SHA256SUMS`, and the Studio release metadata. Source RPMs and debug
   packages stay on GitHub only. Each write carries GitHub's SHA-256 digest
   for the asset, and R2 rejects it unless the bytes match, so the mirror can
   only ever hold GitHub's exact files. Assets already present are skipped.
3. Once every asset is in place, moves `latest` to the new tag.
4. Deletes every mirrored release except the new one and the newest older one.

A run that fails part-way leaves `latest` and the previous release untouched,
and the next run continues where it stopped. A new release therefore reaches
the mirror within about 15 minutes of being published; until then every
client falls back to GitHub.

At roughly 350 MB per release, the two kept releases use about 700 MB of the
account's 10 GB free R2 storage, which the `apexshot-cloud` bucket shares.

## Clients

| Client | Behaviour |
|---|---|
| Website "Download for Windows" (`/download/windows`) | Redirects to the mirror's installer, else GitHub's |
| Linux install/update scripts | `download_file` checks the mirror for the same `<tag>/<name>` and uses it when present, else GitHub. `APEXSHOT_MIRROR_URL=` (empty) forces GitHub |
| Windows in-app updater | Downloads from the mirror first; any failure restarts the download from GitHub |

Release discovery (which version is latest) still reads GitHub in every
client, so GitHub remains the source of truth.

## One-time setup (already done)

- R2 bucket `apexshot-releases`, location automatic, Standard storage.
- Custom domain `dl.apexshot.org` connected to the bucket:
  `npx wrangler r2 bucket domain add apexshot-releases --domain dl.apexshot.org --zone-id <apexshot.org zone id> --min-tls 1.2`
- Worker routes and the cron trigger come from `scripts/cloudflare-wrangler.toml`.

## Deploy

From the repository root:

```bash
npx wrangler deploy --config scripts/cloudflare-wrangler.toml
```

## Check

```bash
curl -fsSL https://dl.apexshot.org/latest
curl -fsSI https://apexshot.org/download/windows | grep -iE '^(HTTP|location|x-apexshot-source)'
npx wrangler r2 object get apexshot-releases/latest --pipe --remote
npx wrangler tail apexshot-install          # watch a cron run's "mirrored …" lines
```

`x-apexshot-source: r2` on `/download/windows` means the mirror served it;
`github` means the mirror did not have the release yet.

## Troubleshooting

| Symptom | Cause |
|---|---|
| `latest` never appears | The cron run fails. `wrangler tail apexshot-install` shows the error; a missing `sha256` digest on an asset stops the run. |
| `/download/windows` says `github` long after a release | Same as above, or the release has no `-setup.exe` asset. |
| `curl https://apexshot.org/install` returns the website | Route not registered. Re-deploy and check Workers → Routes. |
| Site homepage broke | A route pattern is too broad. They must stay `apexshot.org/install*`, `/update*`, and the exact `/download/windows`. |

## Rollback

Delete the Worker's routes in Workers → Routes; `apexshot.org` reverts to its
origin. Clients that read the mirror fall back to GitHub on their own once
the mirror stops answering, so disconnecting `dl.apexshot.org` is also safe.
