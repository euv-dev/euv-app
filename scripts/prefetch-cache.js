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
 * Follow redirects and download a URL, returning the body as a Buffer.
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
          return reject(
            new Error(`HTTP ${res.statusCode} for ${url}`),
          );
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
        stream.on('end', () => resolve(Buffer.concat(chunks)));
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

async function main() {
  console.log('[prefetch-cache] Starting resource prefetch...');
  const config = loadConfig();
  const remoteUrl = config.remote.url;
  const baseUrl = config.remote.baseUrl.replace(/\/$/, '');

  // Clean and recreate bundled cache directory
  if (fs.existsSync(BUNDLED_CACHE_DIR)) {
    fs.rmSync(BUNDLED_CACHE_DIR, { recursive: true });
  }
  fs.mkdirSync(BUNDLED_CACHE_DIR, { recursive: true });

  // Step 1: Download index.html
  console.log(`[prefetch-cache] Downloading HTML: ${remoteUrl}`);
  const htmlBuffer = await downloadUrl(remoteUrl);
  const html = htmlBuffer.toString('utf-8');
  fs.writeFileSync(path.join(BUNDLED_CACHE_DIR, 'index.html'), html);
  console.log('[prefetch-cache] Saved index.html');

  // Step 2: Extract and download linked resources
  const resourcePaths = extractResourcePaths(html);

  // Also add critical resources from config
  if (config.remote.criticalResources) {
    for (const cr of config.remote.criticalResources) {
      if (!resourcePaths.includes(cr)) {
        resourcePaths.push(cr);
      }
    }
  }

  console.log(
    `[prefetch-cache] Found ${resourcePaths.length} resources to download`,
  );

  for (const relPath of resourcePaths) {
    const cleanPath = relPath.replace(/^\.\//, '');
    const url = `${baseUrl}/${cleanPath}`;
    const localPath = path.join(BUNDLED_CACHE_DIR, cleanPath);

    fs.mkdirSync(path.dirname(localPath), { recursive: true });

    try {
      console.log(`[prefetch-cache]   Fetching: ${cleanPath}`);
      const data = await downloadUrl(url);
      fs.writeFileSync(localPath, data);
      console.log(
        `[prefetch-cache]   Saved: ${cleanPath} (${data.length} bytes)`,
      );
    } catch (err) {
      console.warn(
        `[prefetch-cache]   WARN: Failed to fetch ${cleanPath}: ${err.message}`,
      );
    }
  }

  // Step 3: Generate a manifest for Rust to know what's bundled
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
}

main().catch((err) => {
  console.error('[prefetch-cache] Fatal error:', err);
  process.exit(1);
});
