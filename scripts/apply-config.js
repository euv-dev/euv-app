#!/usr/bin/env node
/**
 * apply-config.js
 *
 * 读取 app.config.json，将配置注入到各平台的配置文件中：
 * - tauri.conf.json
 * - dist/index.html
 * - Android strings.xml
 * - Android AppConfig.kt
 *
 * 用法: node scripts/apply-config.js
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
      beforeBuildCommand: 'node scripts/apply-config.js',
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
@keyframes spin{0%{transform:rotate(0deg)}100%{transform:rotate(360deg)}}
.loader{width:48px;height:48px;border:5px solid ${trackColor};border-top:5px solid ${spinnerColor};border-radius:50%;animation:spin 0.8s linear infinite}
</style>
</head><body><div class="loader"></div></body></html>`;
}

/**
 * Generate the index.html that Tauri loads as the frontend entry point.
 * Includes splash screen with the actual project logo (base64 inlined) + spinner,
 * then navigates to the remote URL.
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
        /* Extend into safe areas */
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
      @keyframes spin {
        0% { transform: rotate(0deg); }
        100% { transform: rotate(360deg); }
      }
      .loader {
        width: 36px; height: 36px;
        border: 3px solid ${trackColor};
        border-top: 3px solid ${spinnerColor};
        border-radius: 50%;
        animation: spin 0.8s linear infinite;
      }
      #app { width: 100%; height: 100%; }
    </style>
  </head>
  <body>
    <div id="splash">
      <div id="splash-logo">
        <img src="${logoDataUri}" alt="${config.app.name}" />
      </div>
      <div class="loader"></div>
    </div>
    <div id="app"></div>
    <script>
      (function() {
        var REMOTE_URL = '${config.remote.url}';
        var FADE_DURATION = ${fadeDuration};
        function removeSplash() {
          var s = document.getElementById('splash');
          if (s) {
            s.classList.add('fade-out');
            setTimeout(function() { s.remove(); }, FADE_DURATION);
          }
        }
        removeSplash();
        setTimeout(function() { window.location.replace(REMOTE_URL); }, FADE_DURATION);
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
