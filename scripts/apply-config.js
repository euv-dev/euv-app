#!/usr/bin/env node
/**
 * apply-config.js
 *
 * Reads app.config.json and injects configuration into platform-specific files:
 * - tauri.conf.json
 * - dist/index.html
 * - Android strings.xml
 * - Android AppConfig.kt
 *
 * Usage: node scripts/apply-config.js
 */

const fs = require('fs');
const path = require('path');

const ROOT = path.resolve(__dirname, '..');
const CONFIG_PATH = path.join(ROOT, 'app.config.json');

function loadConfig() {
  if (!fs.existsSync(CONFIG_PATH)) {
    console.error('[ERROR] app.config.json not found at:', CONFIG_PATH);
    process.exit(1);
  }
  return JSON.parse(fs.readFileSync(CONFIG_PATH, 'utf-8'));
}

function writeTauriConf(config) {
  const tauriConfPath = path.join(ROOT, 'src-tauri', 'tauri.conf.json');
  const tauriConf = {
    $schema: 'https://schema.tauri.app/config/2',
    productName: config.app.name,
    version: config.app.version,
    identifier: config.app.identifier,
    build: {
      frontendDist: '../dist',
      beforeDevCommand: '',
      beforeBuildCommand: 'node scripts/prefetch-cache.js && node scripts/apply-config.js',
    },
    app: {
      withGlobalTauri: true,
      windows: [
        {
          title: config.ui.window.title,
          width: config.ui.window.width,
          height: config.ui.window.height,
          resizable: config.ui.window.resizable,
          fullscreen: config.ui.window.fullscreen,
          useHttpsScheme: true,
        },
      ],
      security: {
        csp: null,
      },
    },
    bundle: {
      active: true,
      targets: 'all',
      icon: [
        'icons/32x32.png',
        'icons/128x128.png',
        'icons/128x128@2x.png',
        'icons/icon.ico',
      ],
      android: {
        debugApplicationIdSuffix: '.debug',
      },
    },
  };
  fs.writeFileSync(tauriConfPath, JSON.stringify(tauriConf, null, 2) + '\n');
  console.log('[OK] tauri.conf.json updated');
}

/**
 * Generate the default loading HTML (used as fallback when offline and no cache).
 * Can be overridden by setting ui.loadingHtml in app.config.json.
 */
function getLoadingHtml(config) {
  if (config.ui.loadingHtml) {
    return config.ui.loadingHtml;
  }
  const bg = config.ui.backgroundColor;
  const spinnerColor = config.ui.loadingSpinnerColor || '#1677ff';
  const trackColor = config.ui.loadingSpinnerTrackColor || '#e0e0e0';
  return `<!DOCTYPE html><html><head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width,initial-scale=1.0">
<title>${config.app.name}</title>
<style>body{margin:0;display:flex;align-items:center;justify-content:center;height:100vh;font-family:sans-serif;background:${bg}}
.loader{width:48px;height:48px}
</style>
</head><body><svg class="loader" viewBox="0 0 50 50" xmlns="http://www.w3.org/2000/svg"><circle cx="25" cy="25" r="20" fill="none" stroke="${trackColor}" stroke-width="4"/><circle cx="25" cy="25" r="20" fill="none" stroke="${spinnerColor}" stroke-width="4" stroke-linecap="round" stroke-dasharray="80 200" stroke-dashoffset="0"><animateTransform attributeName="transform" type="rotate" from="0 25 25" to="360 25 25" dur="1s" repeatCount="indefinite"/></circle></svg></body></html>`;
}

/**
 * Generate the index.html that Tauri loads as the frontend entry point.
 *
 * Flow:
 * 1. Show splash/loading screen
 * 2. Invoke `load_cached_resource` to check if local cache exists
 * 3. If cache exists → navigate to euv://localhost/index.html (custom protocol serves from cache)
 * 4. If no cache → show loading spinner while waiting, Rust background task fetches and saves
 *    → poll every 1s until cache becomes available, then navigate
 *
 * Can be overridden by setting ui.indexHtml in app.config.json.
 */
function getIndexHtml(config) {
  if (config.ui.indexHtml) {
    return config.ui.indexHtml;
  }
  const bg = config.ui.backgroundColor;
  const spinnerColor = config.ui.loadingSpinnerColor || '#1677ff';
  const trackColor = config.ui.loadingSpinnerTrackColor || '#e0e0e0';
  const fadeDuration = config.ui.splashFadeDurationMs || 300;

  // Read the splash icon and inline as base64
  const splashIconPath = path.join(
    ROOT,
    'src-tauri',
    'icons',
    'splash-icon.png',
  );
  let logoDataUri = '';
  if (fs.existsSync(splashIconPath)) {
    const iconBase64 = fs.readFileSync(splashIconPath).toString('base64');
    logoDataUri = 'data:image/png;base64,' + iconBase64;
  }

  // Determine the custom protocol URL based on platform
  // Windows/Android: https://euv.localhost/index.html
  // macOS/iOS/Linux: euv://localhost/index.html
  return `<!doctype html>
<html>
  <head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0, maximum-scale=1.0, user-scalable=no, viewport-fit=cover" />
    <title>${config.app.name}</title>
    <style>
      * { margin: 0; padding: 0; box-sizing: border-box; }
      html, body { width: 100%; height: 100%; overflow: hidden; background: ${bg}; }
      #splash {
        position: fixed; top: 0; left: 0; right: 0; bottom: 0;
        display: flex; flex-direction: column; align-items: center; justify-content: center;
        background: ${bg}; z-index: 99999;
        transition: opacity ${fadeDuration}ms ease-out;
        padding-top: env(safe-area-inset-top);
        padding-bottom: env(safe-area-inset-bottom);
        padding-left: env(safe-area-inset-left);
        padding-right: env(safe-area-inset-right);
      }
      #splash.fade-out { opacity: 0; pointer-events: none; }
      #splash-logo {
        width: 120px; height: 120px; margin-bottom: 32px;
      }
      #splash-logo img {
        width: 100%; height: 100%;
        object-fit: contain;
      }
      .loader {
        width: 40px; height: 40px;
      }
      #app { width: 100%; height: 100%; }
    </style>
  </head>
  <body>
    <div id="splash">
      <div id="splash-logo">
        <img src="${logoDataUri}" alt="${config.app.name}" />
      </div>
      <svg class="loader" viewBox="0 0 50 50" xmlns="http://www.w3.org/2000/svg">
        <circle cx="25" cy="25" r="20" fill="none" stroke="${trackColor}" stroke-width="4" />
        <circle cx="25" cy="25" r="20" fill="none" stroke="${spinnerColor}" stroke-width="4" stroke-linecap="round" stroke-dasharray="80 200" stroke-dashoffset="0">
          <animateTransform attributeName="transform" type="rotate" from="0 25 25" to="360 25 25" dur="1s" repeatCount="indefinite" />
        </circle>
      </svg>
    </div>
    <div id="app"></div>
    <script>
      (function() {
        var FADE_DURATION = ${fadeDuration};
        var POLL_INTERVAL = 1000;

        function getSchemeUrl(path) {
          var ua = navigator.userAgent || navigator.platform || '';
          var isWindowsOrAndroid = /Win|Android/.test(ua);
          return isWindowsOrAndroid
            ? 'https://euv.localhost/' + path
            : 'euv://localhost/' + path;
        }

        function navigateToCache() {
          var url = getSchemeUrl('index.html');
          window.location.replace(url);
        }

        function removeSplashAndNavigate() {
          var s = document.getElementById('splash');
          if (s) {
            s.classList.add('fade-out');
            setTimeout(function() { s.remove(); }, FADE_DURATION);
          }
          setTimeout(navigateToCache, 50);
        }

        var pollCount = 0;

        function pollForCache() {
          window.__TAURI_INTERNALS__.invoke('load_cached_resource')
            .then(function(result) {
              if (result && result.from_cache) {
                removeSplashAndNavigate();
              } else {
                pollCount++;
                var delay = pollCount < 3 ? 500 : POLL_INTERVAL;
                setTimeout(pollForCache, delay);
              }
            })
            .catch(function(err) {
              console.warn('[EUV] load_cached_resource error:', err);
              setTimeout(pollForCache, POLL_INTERVAL);
            });
        }

        pollForCache();
      })();
    </script>
  </body>
</html>
`;
}

function writeDistIndex(config) {
  const indexPath = path.join(ROOT, 'dist', 'index.html');
  const html = getIndexHtml(config);
  fs.mkdirSync(path.dirname(indexPath), { recursive: true });
  fs.writeFileSync(indexPath, html);
  console.log('[OK] dist/index.html updated');
}

function writeAndroidStrings(config) {
  const stringsPath = path.join(
    ROOT,
    'src-tauri',
    'gen',
    'android',
    'app',
    'src',
    'main',
    'res',
    'values',
    'strings.xml',
  );
  const xml = `<resources>
    <string name="app_name">${config.app.name}</string>
    <string name="main_activity_title">${config.app.name}</string>
</resources>
`;
  fs.writeFileSync(stringsPath, xml);
  console.log('[OK] Android strings.xml updated');
}

function writeAndroidConfig(config) {
  const configKtPath = path.join(
    ROOT,
    'src-tauri',
    'gen',
    'android',
    'app',
    'src',
    'main',
    'java',
    'com',
    'euv',
    'AppConfig.kt',
  );
  const criticalResources = config.remote.criticalResources
    .map((r) => `        "${r}"`)
    .join(',\n');

  // Generate loading HTML for Android (escaped for Kotlin raw string)
  const loadingHtml = getLoadingHtml(config);
  // Escape $ signs for Kotlin raw strings (triple-quoted)
  const loadingHtmlKt = loadingHtml.replace(/\$/g, '\${"\$"}');

  // Generate index HTML for Android
  const indexHtml = getIndexHtml(config);
  const indexHtmlKt = indexHtml.replace(/\$/g, '\${"\$"}');

  const kt = `package com.euv

/**
 * Auto-generated from app.config.json — do not edit manually.
 * Run: node scripts/apply-config.js
 */
object AppConfig {
    // Remote
    const val REMOTE_URL = "${config.remote.url}"
    const val REMOTE_BASE_URL = "${config.remote.baseUrl}"
    val CRITICAL_SUBRESOURCES = listOf(
${criticalResources}
    )

    // Cache
    const val CACHE_DIR = "${config.cache.directory}"
    const val CONNECT_TIMEOUT_FAST = ${config.cache.connectTimeoutFastMs}
    const val READ_TIMEOUT_FAST = ${config.cache.readTimeoutFastMs}
    const val CONNECT_TIMEOUT_MISS = ${config.cache.connectTimeoutMissMs}
    const val READ_TIMEOUT_MISS = ${config.cache.readTimeoutMissMs}
    const val MAX_REDIRECTS = ${config.cache.maxRedirects}

    // UI
    const val BACKGROUND_COLOR = "${config.ui.backgroundColor}"
    const val SPLASH_FADE_DURATION_MS = ${config.ui.splashFadeDurationMs}L
    const val SPLASH_MAX_WAIT_MS = ${config.ui.splashMaxWaitMs}L
    const val IMMERSIVE_MODE = ${config.ui.immersiveMode}

    // Loading HTML — shown when offline and no cache available
    val LOADING_HTML = """${loadingHtmlKt}"""

    // Index HTML — the frontend entry point loaded by Tauri
    val INDEX_HTML = """${indexHtmlKt}"""

    // Android
    const val KEEP_ALIVE_SERVICE = ${config.android.keepAliveService}
    const val NOTIFICATION_CHANNEL_ID = "${config.android.notification.channelId}"
    const val NOTIFICATION_CHANNEL_NAME = "${config.android.notification.channelName}"
    const val NOTIFICATION_TITLE = "${config.android.notification.title}"
    const val NOTIFICATION_TEXT = "${config.android.notification.text}"
}
`;
  fs.mkdirSync(path.dirname(configKtPath), { recursive: true });
  fs.writeFileSync(configKtPath, kt);
  console.log('[OK] Android AppConfig.kt generated');
}

function writeAndroidBuildGradle(config) {
  const gradlePath = path.join(
    ROOT,
    'src-tauri',
    'gen',
    'android',
    'app',
    'build.gradle.kts',
  );
  if (!fs.existsSync(gradlePath)) {
    return;
  }
  let gradle = fs.readFileSync(gradlePath, 'utf-8');
  const targetSdk = config.android.targetSdk || 35;
  const compileSdk = config.android.compileSdk || 35;
  const minSdk = config.android.minSdk || 24;
  gradle = gradle.replace(/compileSdk\s*=\s*\d+/, `compileSdk = ${compileSdk}`);
  gradle = gradle.replace(/minSdk\s*=\s*\d+/, `minSdk = ${minSdk}`);
  gradle = gradle.replace(/targetSdk\s*=\s*\d+/, `targetSdk = ${targetSdk}`);
  fs.writeFileSync(gradlePath, gradle);
  console.log(
    `[OK] Android build.gradle.kts SDK versions updated (compileSdk=${compileSdk}, minSdk=${minSdk}, targetSdk=${targetSdk})`,
  );
}

function main() {
  console.log('[apply-config] Reading app.config.json...');
  const config = loadConfig();
  console.log(`[apply-config] App: ${config.app.name} v${config.app.version}`);

  writeTauriConf(config);
  writeDistIndex(config);
  writeAndroidStrings(config);
  writeAndroidConfig(config);
  writeAndroidBuildGradle(config);

  console.log('[apply-config] Done! All platform configs updated.');
}

main();
