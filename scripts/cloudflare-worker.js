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
 * Windows installer on GitHub (see DOWNLOADS).
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

/**
 * Public URL path -> suffix of the release asset it redirects to. The file is
 * never proxied or mirrored: the Worker looks up the latest GitHub release and
 * answers with a redirect to the asset's own download URL, so the website gets
 * a stable link and the in-app updater keeps reading the same release.
 */
const DOWNLOADS = {
  '/download/windows': { suffix: '-setup.exe', distro: 'windows', assetType: 'windows_installer' },
};

/**
 * Download telemetry, the same endpoint the Linux install scripts report to.
 * A redirect only proves the download started (the browser fetches the file
 * from GitHub), so these are recorded as `download_started`.
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

function redirect(location, cacheControl) {
  return new Response(null, {
    status: 302,
    headers: { location, 'cache-control': cacheControl },
  });
}

/**
 * Redirect to the latest release's asset ending in `suffix`. The GitHub API
 * response is cached at the edge for five minutes so downloads do not spend
 * the unauthenticated rate limit; any failure falls back to the release page.
 */
async function downloadRedirect(download, request, ctx) {
  const { suffix } = download;
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
        return redirect(asset.browser_download_url, 'public, max-age=300');
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

export default {
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
      return downloadRedirect(download, request, ctx);
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
