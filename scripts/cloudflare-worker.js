/**
 * ApexShot installer proxy.
 *
 * Serves the install/update entrypoints on apexshot.org so users type
 *
 *     curl -fsSL https://apexshot.org/install | sh
 *
 * instead of the raw.githubusercontent.com URL.
 *
 * Resolution order for every script:
 *   1. R2 bucket (binding: RELEASES) at key `scripts/<file>`
 *   2. GitHub `main` fallback (raw.githubusercontent.com)
 *
 * R2-first means the entrypoint keeps working even when GitHub is slow,
 * throttled, or unreachable in a user's region. The GitHub fallback means a
 * failed or skipped R2 script sync in CI can never brick installs.
 *
 * It also serves `/download/windows`, a redirect to the latest release's
 * Windows installer (see DOWNLOADS): the R2 mirror's copy when it has one,
 * otherwise the GitHub asset.
 *
 * Release mirror. A cron trigger copies the latest GitHub release's
 * user-facing assets into R2 under `<tag>/<name>`, which the bucket's custom
 * domain serves as https://dl.apexshot.org/<tag>/<name>. Each object is
 * checked against the SHA-256 digest GitHub reports, so the mirror can only
 * ever hold GitHub's exact bytes. Once every asset is in place the `latest`
 * pointer moves to the new tag, and every release except that one and the
 * newest older one is deleted, so the bucket stays small without anyone
 * cleaning it up.
 *
 * Only the paths in ROUTES and DOWNLOADS are handled; every other request on the zone
 * falls through to the normal apexshot.org origin untouched.
 *
 * Config file: scripts/cloudflare-wrangler.toml
 * Deploy:      npx wrangler deploy --config scripts/cloudflare-wrangler.toml
 */

const GH_RAW = 'https://raw.githubusercontent.com/apex-shot/apexshot/main/scripts';
const GH_LATEST_API = 'https://api.github.com/repos/apex-shot/apexshot/releases/latest';
const GH_RELEASES_PAGE = 'https://github.com/apex-shot/apexshot/releases/latest';

/** Public origin of the R2 bucket's custom domain. */
const MIRROR_ORIGIN = 'https://dl.apexshot.org';
/** Releases kept in R2: the latest and the one before it. */
const MIRROR_KEEP = 2;
/** Plain text object naming the newest fully mirrored tag. */
const MIRROR_LATEST_KEY = 'latest';
const RELEASE_TAG = /^v\d+(\.\d+){1,3}$/;

/**
 * Only what installers and the website download is mirrored. Source RPMs and
 * debug packages stay on GitHub, which keeps a release near 350 MB in R2.
 */
function isMirroredAsset(name) {
  return !/\.src\.rpm$|debuginfo|debugsource|^apexshot-debug-/.test(name);
}

/**
 * Public URL path -> suffix of the release asset it redirects to. The Worker
 * answers with a redirect, never the file itself: to the R2 mirror's copy of
 * the latest mirrored release, or to the latest GitHub release's asset when
 * the mirror does not have one.
 */
const DOWNLOADS = {
  '/download/windows': { suffix: '-setup.exe', distro: 'windows', assetType: 'windows_installer' },
};

/**
 * Download telemetry, the same endpoint the Linux install scripts report to.
 * A redirect only proves the download started (the browser fetches the file
 * itself), so these are recorded as `download_started`.
 */
const TELEMETRY_URL = 'https://apexshot.org/api/download-telemetry';
const BOT_UA = /bot|crawl|spider|slurp|preview|facebookexternalhit|embedly|headless|lighthouse|monitor|uptime/i;

/** Public URL path -> script filename inside `scripts/`. */
const ROUTES = {
  '/install': 'install.sh',
  '/update': 'update.sh',

  '/install/ubuntu': 'ubuntu-install.sh',
  '/install/arch': 'arch-install.sh',
  '/install/fedora': 'fedora-install.sh',

  '/update/ubuntu': 'ubuntu-update.sh',
  '/update/arch': 'arch-update.sh',
  '/update/fedora': 'fedora-update.sh',
};

const SCRIPT_HEADERS = {
  'content-type': 'text/x-shellscript; charset=utf-8',
  'x-content-type-options': 'nosniff',
};

/** Normalise `/install/` -> `/install`. */
function normalisePath(pathname) {
  if (pathname.length > 1 && pathname.endsWith('/')) {
    return pathname.slice(0, -1);
  }
  return pathname;
}

function scriptResponse(body, source, cacheControl) {
  return new Response(body, {
    status: 200,
    headers: { ...SCRIPT_HEADERS, 'cache-control': cacheControl, 'x-apexshot-source': source },
  });
}

function redirect(location, cacheControl, source) {
  const headers = { location, 'cache-control': cacheControl };
  if (source) headers['x-apexshot-source'] = source;
  return new Response(null, { status: 302, headers });
}

/** The mirrored object of the latest mirrored release ending in `suffix`. */
async function mirroredAsset(env, suffix) {
  const pointer = await env.RELEASES.get(MIRROR_LATEST_KEY);
  if (!pointer) return null;
  const tag = (await pointer.text()).trim();
  if (!RELEASE_TAG.test(tag)) return null;
  const listing = await env.RELEASES.list({ prefix: `${tag}/` });
  const object = listing.objects.find((candidate) => candidate.key.endsWith(suffix));
  if (!object) return null;
  return {
    release: { tag_name: tag },
    asset: { name: object.key.slice(tag.length + 1), size: object.size },
    url: `${MIRROR_ORIGIN}/${object.key}`,
  };
}

/**
 * Redirect to the latest release's asset ending in `suffix`: the R2 mirror
 * first, then GitHub. The GitHub API response is cached at the edge for five
 * minutes so downloads do not spend the unauthenticated rate limit; any
 * failure falls back to the release page.
 */
async function downloadRedirect(download, request, env, ctx) {
  const { suffix } = download;
  try {
    const mirrored = await mirroredAsset(env, suffix);
    if (mirrored) {
      if (request.method === 'GET') {
        ctx.waitUntil(recordDownload(download, mirrored.release, mirrored.asset, request));
      }
      return redirect(mirrored.url, 'public, max-age=60', 'r2');
    }
  } catch (err) {
    // An R2 hiccup must never break the download; GitHub still has the file.
    console.error(`mirror lookup failed: ${err}`);
  }
  try {
    const upstream = await fetch(GH_LATEST_API, {
      headers: {
        accept: 'application/vnd.github+json',
        'user-agent': 'apexshot-install-worker',
      },
      cf: { cacheTtl: 300, cacheEverything: true },
    });
    if (upstream.ok) {
      const release = await upstream.json();
      const asset = (release.assets || []).find((candidate) =>
        candidate.name.endsWith(suffix),
      );
      if (asset) {
        if (request.method === 'GET') {
          ctx.waitUntil(recordDownload(download, release, asset, request));
        }
        return redirect(asset.browser_download_url, 'public, max-age=60', 'github');
      }
      console.error(`latest release ${release.tag_name} has no *${suffix} asset`);
    } else {
      console.error(`latest release lookup returned ${upstream.status}`);
    }
  } catch (err) {
    console.error(`latest release lookup failed: ${err}`);
  }
  return redirect(GH_RELEASES_PAGE, 'no-store');
}

/** Best effort: telemetry must never delay or break the download. */
async function recordDownload(download, release, asset, request) {
  const userAgent = request.headers.get('user-agent') || '';
  if (!userAgent || BOT_UA.test(userAgent)) return;
  try {
    await fetch(TELEMETRY_URL, {
      method: 'POST',
      headers: { 'content-type': 'application/json', 'user-agent': userAgent.slice(0, 255) },
      body: JSON.stringify({
        event: 'download_started',
        script: 'website',
        distro: download.distro,
        channel: 'install',
        version: String(release.tag_name || '').replace(/^v/i, '') || null,
        asset_type: download.assetType,
        asset_name: asset.name,
        status: 'redirected',
        size_bytes: typeof asset.size === 'number' ? asset.size : null,
        install_id: null,
      }),
    });
  } catch (err) {
    console.error(`download telemetry failed: ${err}`);
  }
}

function compareTags(a, b) {
  const left = a.slice(1).split('.').map(Number);
  const right = b.slice(1).split('.').map(Number);
  for (let index = 0; index < Math.max(left.length, right.length); index += 1) {
    const difference = (left[index] || 0) - (right[index] || 0);
    if (difference !== 0) return difference;
  }
  return 0;
}

/**
 * Copy one GitHub asset into R2. The stream is never buffered: GitHub's body
 * is piped through a fixed-length stream so R2 knows the size up front, and
 * R2 rejects the write unless the bytes match GitHub's SHA-256 digest.
 */
async function mirrorAsset(env, tag, asset) {
  const key = `${tag}/${asset.name}`;
  const existing = await env.RELEASES.head(key);
  if (existing && existing.size === asset.size) return false;

  const digest = typeof asset.digest === 'string' && asset.digest.startsWith('sha256:')
    ? asset.digest.slice('sha256:'.length)
    : null;
  if (!digest) throw new Error(`${asset.name} has no sha256 digest`);

  const upstream = await fetch(asset.browser_download_url, {
    headers: { 'user-agent': 'apexshot-release-mirror' },
  });
  if (!upstream.ok || !upstream.body) {
    throw new Error(`${asset.name}: GitHub answered ${upstream.status}`);
  }
  const { readable, writable } = new FixedLengthStream(asset.size);
  const piped = upstream.body.pipeTo(writable);
  await env.RELEASES.put(key, readable, {
    sha256: digest,
    httpMetadata: {
      contentType: asset.content_type || 'application/octet-stream',
      contentDisposition: `attachment; filename="${asset.name}"`,
      cacheControl: 'public, max-age=31536000, immutable',
    },
  });
  await piped;
  console.log(`mirrored ${key} (${asset.size} bytes)`);
  return true;
}

/** Delete every mirrored release except `keep`. */
async function pruneReleases(env, keep) {
  const tags = [];
  let cursor;
  do {
    const listing = await env.RELEASES.list({ delimiter: '/', cursor });
    for (const prefix of listing.delimitedPrefixes) {
      const tag = prefix.slice(0, -1);
      if (RELEASE_TAG.test(tag)) tags.push(tag);
    }
    cursor = listing.truncated ? listing.cursor : undefined;
  } while (cursor);

  for (const tag of tags) {
    if (keep.includes(tag)) continue;
    let objectCursor;
    do {
      const listing = await env.RELEASES.list({ prefix: `${tag}/`, cursor: objectCursor });
      const keys = listing.objects.map((object) => object.key);
      if (keys.length) await env.RELEASES.delete(keys);
      objectCursor = listing.truncated ? listing.cursor : undefined;
    } while (objectCursor);
    console.log(`pruned ${tag}`);
  }
}

/**
 * Mirror the latest GitHub release, move `latest` to it, and prune. Nothing is
 * pruned and `latest` does not move unless every asset of the new release is
 * in R2, so a failed run leaves the previous release serving and the next
 * run picks up where this one stopped.
 */
async function syncMirror(env) {
  const response = await fetch(GH_LATEST_API, {
    headers: {
      accept: 'application/vnd.github+json',
      'user-agent': 'apexshot-release-mirror',
    },
  });
  if (!response.ok) throw new Error(`latest release lookup returned ${response.status}`);
  const release = await response.json();
  const tag = String(release.tag_name || '');
  if (!RELEASE_TAG.test(tag) || release.draft || release.prerelease) {
    throw new Error(`refusing to mirror release ${JSON.stringify(tag)}`);
  }

  const assets = (release.assets || []).filter((asset) => isMirroredAsset(asset.name));
  if (!assets.length) throw new Error(`${tag} has no assets to mirror`);
  for (const asset of assets) {
    await mirrorAsset(env, tag, asset);
  }

  const pointer = await env.RELEASES.get(MIRROR_LATEST_KEY);
  const previous = pointer ? (await pointer.text()).trim() : '';
  if (previous !== tag) {
    await env.RELEASES.put(MIRROR_LATEST_KEY, tag, {
      httpMetadata: { contentType: 'text/plain; charset=utf-8', cacheControl: 'public, max-age=60' },
    });
    console.log(`latest is now ${tag}`);
  }

  // Keep the release just published and the newest one older than it.
  const older = [];
  const listing = await env.RELEASES.list({ delimiter: '/' });
  for (const prefix of listing.delimitedPrefixes) {
    const candidate = prefix.slice(0, -1);
    if (RELEASE_TAG.test(candidate) && compareTags(candidate, tag) < 0) older.push(candidate);
  }
  older.sort(compareTags);
  const keep = [tag, ...older.slice(-(MIRROR_KEEP - 1))];
  await pruneReleases(env, keep);
}

export default {
  async scheduled(event, env, ctx) {
    await syncMirror(env);
  },

  async fetch(request, env, ctx) {
    if (request.method !== 'GET' && request.method !== 'HEAD') {
      return new Response('method not allowed\n', {
        status: 405,
        headers: { allow: 'GET, HEAD', 'content-type': 'text/plain; charset=utf-8' },
      });
    }

    const url = new URL(request.url);
    const path = normalisePath(url.pathname);

    const download = DOWNLOADS[path];
    if (download) {
      return downloadRedirect(download, request, env, ctx);
    }

    const file = ROUTES[path];

    if (!file) {
      return new Response('unknown installer path\n', {
        status: 404,
        headers: { 'content-type': 'text/plain; charset=utf-8' },
      });
    }

    // 1. R2 mirror. Short edge TTL so a bad script release can be rolled back
    //    within minutes rather than waiting out a long cache.
    try {
      const object = await env.RELEASES.get(`scripts/${file}`);
      if (object) {
        return scriptResponse(object.body, 'r2', 'public, max-age=300');
      }
    } catch (err) {
      // Never fail the request on an R2 hiccup; fall through to GitHub.
      console.error(`r2 read failed for ${file}: ${err}`);
    }

    // 2. GitHub fallback.
    try {
      const upstream = await fetch(`${GH_RAW}/${file}`, {
        cf: { cacheTtl: 300, cacheEverything: true },
      });
      if (upstream.ok) {
        return scriptResponse(upstream.body, 'github', 'public, max-age=300');
      }
      console.error(`github fallback ${upstream.status} for ${file}`);
    } catch (err) {
      console.error(`github fallback failed for ${file}: ${err}`);
    }

    return new Response('installer temporarily unavailable\n', {
      status: 502,
      headers: { 'content-type': 'text/plain; charset=utf-8', 'retry-after': '60' },
    });
  },
};
