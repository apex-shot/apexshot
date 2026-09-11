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
 * Only the paths in ROUTES are handled; every other request on the zone
 * falls through to the normal apexshot.org origin untouched.
 *
 * Config file: scripts/cloudflare-wrangler.toml
 * Deploy:      npx wrangler deploy --config scripts/cloudflare-wrangler.toml
 */

const GH_RAW = 'https://raw.githubusercontent.com/apex-shot/apexshot/main/scripts';

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

export default {
  async fetch(request, env) {
    if (request.method !== 'GET' && request.method !== 'HEAD') {
      return new Response('method not allowed\n', {
        status: 405,
        headers: { allow: 'GET, HEAD', 'content-type': 'text/plain; charset=utf-8' },
      });
    }

    const url = new URL(request.url);
    const file = ROUTES[normalisePath(url.pathname)];

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
