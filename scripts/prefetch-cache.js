#!/usr/bin/env node
/**
 * prefetch-cache.js
 *
 * Downloads remote resources at build time and saves them into
 * src-tauri/bundled-cache/ so they are embedded into the binary.
 * This ensures the app can start offline on first launch.
 *
 * Usage: node scripts/prefetch-cache.js
 */

const fs = require('fs');
const path = require('path');
const https = require('https');
const http = require('http');
const zlib = require('zlib');

const ROOT = path.resolve(__dirname, '..');
const CONFIG_PATH = path.join(ROOT, 'app.config.json');
const BUNDLED_CACHE_DIR = path.join(ROOT, 'src-tauri', 'bundled-cache');

function loadConfig() {
  if (!fs.existsSync(CONFIG_PATH)) {
    console.error('[ERROR] app.config.json not found');
    process.exit(1);
  }
  return JSON.parse(fs.readFileSync(CONFIG_PATH, 'utf-8'));
}

/**
 * Follow redirects and download a URL, returning { finalUrl, body } where
 * finalUrl is the URL after all redirects and body is a Buffer.
 */
function downloadUrl(url, maxRedirects = 10) {
  return new Promise((resolve, reject) => {
    const client = url.startsWith('https') ? https : http;
    const options = {
      headers: { 'Accept-Encoding': 'gzip, deflate, identity' },
    };
    client
      .get(url, options, (res) => {
        if (
          res.statusCode >= 300 &&
          res.statusCode < 400 &&
          res.headers.location
        ) {
          if (maxRedirects <= 0) {
            return reject(new Error('Too many redirects'));
          }
          let redirectUrl = res.headers.location;
          if (redirectUrl.startsWith('/')) {
            const parsed = new URL(url);
            redirectUrl = `${parsed.protocol}//${parsed.host}${redirectUrl}`;
          }
          return resolve(downloadUrl(redirectUrl, maxRedirects - 1));
        }
        if (res.statusCode !== 200) {
          return reject(new Error(`HTTP ${res.statusCode} for ${url}`));
        }
        // Handle content-encoding (decompress if needed)
        let stream = res;
        const encoding = (res.headers['content-encoding'] || '').toLowerCase();
        if (encoding === 'gzip') {
          stream = res.pipe(zlib.createGunzip());
        } else if (encoding === 'deflate') {
          stream = res.pipe(zlib.createInflate());
        } else if (encoding === 'br') {
          stream = res.pipe(zlib.createBrotliDecompress());
        }
        const chunks = [];
        stream.on('data', (chunk) => chunks.push(chunk));
        stream.on('end', () =>
          resolve({ finalUrl: url, body: Buffer.concat(chunks) }),
        );
        stream.on('error', reject);
        res.on('error', reject);
      })
      .on('error', reject);
  });
}

/**
 * Extract relative resource paths from HTML (script src, link href, img src).
 */
function extractResourcePaths(html) {
  const paths = [];
  const patterns = [
    /< *script[^>]+src=["']([^"']+)["']/gi,
    /< *link[^>]+href=["']([^"']+)["']/gi,
    /< *img[^>]+src=["']([^"']+)["']/gi,
  ];
  for (const pattern of patterns) {
    let match;
    while ((match = pattern.exec(html)) !== null) {
      const val = match[1];
      // Skip absolute URLs and data URIs
      if (
        val.startsWith('http://') ||
        val.startsWith('https://') ||
        val.startsWith('//') ||
        val.startsWith('data:')
      ) {
        continue;
      }
      paths.push(val);
    }
  }
  // Also extract ES module imports: import ... from './path'
  const importPattern = /import\s+.*?from\s+['"](\.[^'"]+)['"]/g;
  let m;
  while ((m = importPattern.exec(html)) !== null) {
    paths.push(m[1]);
  }
  return [...new Set(paths)];
}

/**
 * Derive the base URL for resolving relative resource paths.
 * If the URL ends with '/', it's a directory — use it (minus trailing slash).
 * Otherwise, strip the last path segment.
 * e.g. "https://ltpp.vip/github/pages/euv-dev/euv/" -> "https://ltpp.vip/github/pages/euv-dev/euv"
 *      "https://ltpp.vip/euv" -> "https://ltpp.vip"
 */
function deriveBaseUrl(finalUrl) {
  const trimmed = finalUrl.replace(/\/$/, '');
  if (finalUrl.endsWith('/')) {
    return trimmed;
  }
  const lastSlash = trimmed.lastIndexOf('/');
  const schemeEnd = trimmed.indexOf('://');
  if (lastSlash > schemeEnd + 2) {
    return trimmed.slice(0, lastSlash);
  }
  return trimmed;
}

/**
 * Extract dependency references (.wasm and relative ES imports) from JS content.
 *
 * Each discovered dependency is resolved against the JS file's FINAL URL (after
 * redirects) so that the resulting download URL is correct even when the JS file
 * itself was served from a redirected location. It also computes the local cache
 * path relative to the cache root, derived from the JS file's clean cache path.
 *
 * @param {string} jsContent  - The JS source code.
 * @param {string} jsFinalUrl - The final URL of the JS file (after redirects).
 * @param {string} jsCleanPath - The JS file's path relative to the cache root.
 * @returns {Array<{url: string, cleanPath: string}>} resolved dependencies.
 */
function extractJsDependencies(jsContent, jsFinalUrl, jsCleanPath) {
  const refs = [];
  // 1) .wasm string literals, e.g. new URL('euv_bg.wasm', import.meta.url)
  const wasmPattern = /['"]([^'"]*\.wasm)['"]/g;
  let match;
  while ((match = wasmPattern.exec(jsContent)) !== null) {
    refs.push(match[1]);
  }
  // 2) static/dynamic relative ES module imports
  const importPattern =
    /(?:import\s+[^'"]*?from\s*|import\s*\(\s*)['"](\.[^'"]+)['"]/g;
  while ((match = importPattern.exec(jsContent)) !== null) {
    refs.push(match[1]);
  }

  const results = [];
  const seen = new Set();
  const jsCacheDir = path.posix.dirname(jsCleanPath); // dir within cache root
  for (const ref of refs) {
    if (ref.includes('://') || ref.startsWith('//') || ref.includes(' ')) {
      continue;
    }
    // Resolve the absolute download URL against the JS file's FINAL url.
    const absoluteUrl = new URL(ref, jsFinalUrl).toString();
    // Resolve the local cache path against the JS file's cache directory.
    const stripped = ref.replace(/^\.\//, '');
    const cleanPath =
      jsCacheDir === '.' || jsCacheDir === ''
        ? stripped
        : path.posix.normalize(`${jsCacheDir}/${stripped}`);
    if (!seen.has(cleanPath)) {
      seen.add(cleanPath);
      results.push({ url: absoluteUrl, cleanPath });
    }
  }
  return results;
}

async function main() {
  console.log('[prefetch-cache] Starting resource prefetch...');
  const config = loadConfig();
  const remoteUrl = config.remote.url;

  // Clean and recreate bundled cache directory
  if (fs.existsSync(BUNDLED_CACHE_DIR)) {
    fs.rmSync(BUNDLED_CACHE_DIR, { recursive: true });
  }
  fs.mkdirSync(BUNDLED_CACHE_DIR, { recursive: true });

  // Step 1: Download index.html (following redirects, use final URL as base)
  console.log(`[prefetch-cache] Downloading HTML: ${remoteUrl}`);
  const { finalUrl, body: htmlBuffer } = await downloadUrl(remoteUrl);
  let html = htmlBuffer.toString('utf-8');
  // euv-app immersive patch. The downloaded euv Pages build does NOT set
  // `viewport-fit=cover` on its <meta name="viewport"> tag (euv master
  // template PR #59 removed it for the regular web target), which means
  // CSS `env(safe-area-inset-top)` returns 0px on Android. Without that
  // signal, `IMMERSIVE_SAFE_AREA_SCRIPT.applyInset()` writes
  // `--euv-mobile-safe-top: 0px` and `c_mobile_header`'s `padding-top`
  // collapses onto the status bar. We add `viewport-fit=cover` here at
  // prefetch time so the page is parsed with the cover-fit flag from the
  // very first byte — runtime meta rewrites via JS are too late because
  // the browser locks the viewport-fit value at parse time.
  html = html.replace(
    /<meta\s+name="viewport"\s+content="([^"]*)"\s*\/>/,
    (match, content) => {
      if (!/viewport-fit\s*=\s*cover/.test(content)) {
        return `<meta name="viewport" content="${content}, viewport-fit=cover" />`;
      }
      return match;
    },
  );
  // Redirect to the form demo on launch so the IME bridge and the
  // framework's focus-scroll fix are immediately visible. The downloaded
  // euv Pages build opens the static About screen, which has no inputs.
  if (!/window\.location\.hash\s*=\s*['"]#\/form['"]/.test(html)) {
    const redirectScript =
      '<script>(function(){if(!/^#\/form$/.test(window.location.hash))' +
      '{window.location.hash="#/form";}})();</script>';
    if (/<\/head>/i.test(html)) {
      html = html.replace(/<\/head>/i, redirectScript + '</head>');
    } else if (/<body[^>]*>/i.test(html)) {
      html = html.replace(/(<body[^>]*>)/i, '$1' + redirectScript);
    } else {
      html = redirectScript + html;
    }
  }
  fs.writeFileSync(path.join(BUNDLED_CACHE_DIR, 'index.html'), html);
  console.log(`[prefetch-cache] Final URL after redirects: ${finalUrl}`);
  const baseUrl = deriveBaseUrl(finalUrl);
  console.log(`[prefetch-cache] Base URL for resources: ${baseUrl}`);

  // Optional verification mode: bundle ONLY index.html and skip all other
  // resources (JS/wasm). This is used to verify that the runtime "fetch update"
  // path can pull the full snapshot over the network on its own. Enable via
  // the PREFETCH_INDEX_ONLY=1 environment variable.
  const indexOnly =
    process.env.PREFETCH_INDEX_ONLY === '1' ||
    process.env.PREFETCH_INDEX_ONLY === 'true';
  if (indexOnly) {
    console.log(
      '[prefetch-cache] PREFETCH_INDEX_ONLY enabled: bundling index.html only',
    );
    const manifest = {
      version: config.app.version,
      timestamp: new Date().toISOString(),
      files: ['index.html'],
    };
    fs.writeFileSync(
      path.join(BUNDLED_CACHE_DIR, '_manifest.json'),
      JSON.stringify(manifest, null, 2),
    );
    console.log(
      '[prefetch-cache] Done! 1 file bundled (index.html only) into src-tauri/bundled-cache/',
    );
    return;
  }

  // Step 2: Extract and download linked resources from HTML
  const resourcePaths = extractResourcePaths(html);

  console.log(
    `[prefetch-cache] Found ${resourcePaths.length} resources to download`,
  );

  const downloadedFiles = []; // { cleanPath, finalUrl, data }
  const fetchedPaths = new Set();

  for (const relPath of resourcePaths) {
    const cleanPath = relPath.replace(/^\.\//, '');
    const url = `${baseUrl}/${cleanPath}`;
    const localPath = path.join(BUNDLED_CACHE_DIR, cleanPath);

    fs.mkdirSync(path.dirname(localPath), { recursive: true });

    try {
      console.log(`[prefetch-cache]   Fetching: ${cleanPath}`);
      // Capture the resource's OWN final URL after redirects so that any
      // dependencies discovered inside it resolve against the correct base.
      const { finalUrl: resFinalUrl, body: data } = await downloadUrl(url);
      if (data.length === 0) {
        console.warn(
          `[prefetch-cache]   WARN: Empty response for ${cleanPath}, skipping`,
        );
        continue;
      }
      fs.writeFileSync(localPath, data);
      console.log(
        `[prefetch-cache]   Saved: ${cleanPath} (${data.length} bytes)`,
      );
      if (resFinalUrl !== url) {
        console.log(`[prefetch-cache]     (redirected to: ${resFinalUrl})`);
      }
      fetchedPaths.add(cleanPath);
      downloadedFiles.push({ cleanPath, finalUrl: resFinalUrl, data });
    } catch (err) {
      console.warn(
        `[prefetch-cache]   WARN: Failed to fetch ${cleanPath}: ${err.message}`,
      );
    }
  }

  // Step 3: Scan downloaded JS for additional dependencies (.wasm, relative
  // imports). Each dependency is resolved against the JS file's FINAL url so
  // redirected resources still produce correct download URLs.
  const extraDeps = []; // { url, cleanPath }
  const extraSeen = new Set();
  for (const { cleanPath, finalUrl: resFinalUrl, data } of downloadedFiles) {
    if (cleanPath.endsWith('.js') || cleanPath.endsWith('.mjs')) {
      const jsContent = data.toString('utf-8');
      const deps = extractJsDependencies(jsContent, resFinalUrl, cleanPath);
      for (const dep of deps) {
        if (!fetchedPaths.has(dep.cleanPath) && !extraSeen.has(dep.cleanPath)) {
          extraSeen.add(dep.cleanPath);
          extraDeps.push(dep);
        }
      }
    }
  }

  if (extraDeps.length > 0) {
    console.log(
      `[prefetch-cache] Found ${extraDeps.length} extra dependencies in JS`,
    );
    for (const { url, cleanPath } of extraDeps) {
      const localPath = path.join(BUNDLED_CACHE_DIR, cleanPath);

      fs.mkdirSync(path.dirname(localPath), { recursive: true });

      try {
        console.log(`[prefetch-cache]   Fetching: ${cleanPath}`);
        const { finalUrl: depFinalUrl, body: data } = await downloadUrl(url);
        if (data.length === 0) {
          console.warn(
            `[prefetch-cache]   WARN: Empty response for ${cleanPath}, skipping`,
          );
          continue;
        }
        fs.writeFileSync(localPath, data);
        console.log(
          `[prefetch-cache]   Saved: ${cleanPath} (${data.length} bytes)`,
        );
        if (depFinalUrl !== url) {
          console.log(`[prefetch-cache]     (redirected to: ${depFinalUrl})`);
        }
      } catch (err) {
        console.warn(
          `[prefetch-cache]   WARN: Failed to fetch ${cleanPath}: ${err.message}`,
        );
      }
    }
  }

  // Step 4: Generate a manifest for Rust to know what's bundled
  const manifest = {
    version: config.app.version,
    timestamp: new Date().toISOString(),
    files: [],
  };

  function walkDir(dir, prefix = '') {
    const entries = fs.readdirSync(dir, { withFileTypes: true });
    for (const entry of entries) {
      const fullPath = path.join(dir, entry.name);
      const relName = prefix ? `${prefix}/${entry.name}` : entry.name;
      if (entry.isDirectory()) {
        walkDir(fullPath, relName);
      } else {
        manifest.files.push(relName);
      }
    }
  }
  walkDir(BUNDLED_CACHE_DIR);

  fs.writeFileSync(
    path.join(BUNDLED_CACHE_DIR, '_manifest.json'),
    JSON.stringify(manifest, null, 2),
  );

  console.log(
    `[prefetch-cache] Done! ${manifest.files.length} files bundled into src-tauri/bundled-cache/`,
  );
  // Node.js http/https agents keep sockets alive (Connection: keep-alive),
  // which prevents the event loop from becoming empty. Force-exit so the
  // build pipeline does not hang after all files are written.
  
  // euv-app override: force soft-keyboard demo route on launch.
  // The downloaded index.html opens the static About screen (no inputs),
  // which makes the IME/keyboard fix invisible. We replace it with a copy
  // from src-tauri/bundled-cache/_euv_app_index.html that injects a hash
  // redirect to /form before the WASM bundle loads.
  const localIndex = path.join(BUNDLED_CACHE_DIR, '_euv_app_index.html');
  if (fs.existsSync(localIndex)) {
    fs.copyFileSync(localIndex, path.join(BUNDLED_CACHE_DIR, 'index.html'));
    console.log('[prefetch-cache] Replaced index.html with euv-app override.');
  }

process.exit(0);
}

main().catch((err) => {
  console.error('[prefetch-cache] Fatal error:', err);
  process.exit(1);
});
