#!/usr/bin/env node
/**
 * apply-config.js
 * 
 * 读取 app.config.json，将配置注入到各平台的配置文件中：
 * - tauri.conf.json
 * - dist/index.html
 * - Android strings.xml
 * - Rust const.rs
 * - Android AppConfig.kt
 * 
 * 用法: node scripts/apply-config.js
 */

const fs = require('fs');
const path = require('path');

const ROOT = path.resolve(__dirname, '..');
const CONFIG_PATH = path.join(ROOT, 'app.config.json');

function loadConfig () {
  if (!fs.existsSync(CONFIG_PATH)) {
    console.error('[ERROR] app.config.json not found at:', CONFIG_PATH);
    process.exit(1);
  }
  return JSON.parse(fs.readFileSync(CONFIG_PATH, 'utf-8'));
}

function writeTauriConf (config) {
  const tauriConfPath = path.join(ROOT, 'src-tauri', 'tauri.conf.json');
  const tauriConf = {
    "$schema": "https://schema.tauri.app/config/2",
    "productName": config.app.name,
    "version": config.app.version,
    "identifier": config.app.identifier,
    "build": {
      "frontendDist": "../dist",
      "beforeDevCommand": "",
      "beforeBuildCommand": "node scripts/apply-config.js"
    },
    "app": {
      "windows": [
        {
          "title": config.ui.window.title,
          "width": config.ui.window.width,
          "height": config.ui.window.height,
          "resizable": config.ui.window.resizable,
          "fullscreen": config.ui.window.fullscreen
        }
      ],
      "security": {
        "csp": null
      }
    },
    "bundle": {
      "active": true,
      "targets": "all",
      "icon": [
        "icons/32x32.png",
        "icons/128x128.png",
        "icons/128x128@2x.png",
        "icons/icon.icns",
        "icons/icon.ico"
      ],
      "android": {
        "debugApplicationIdSuffix": ".debug"
      }
    }
  };
  fs.writeFileSync(tauriConfPath, JSON.stringify(tauriConf, null, 2) + '\n');
  console.log('[OK] tauri.conf.json updated');
}

/**
 * Generate the default loading HTML (used as fallback when offline and no cache).
 * Can be overridden by setting ui.loadingHtml in app.config.json.
 */
function getLoadingHtml (config) {
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
 * Can be overridden by setting ui.indexHtml in app.config.json.
 */
function getIndexHtml (config) {
  if (config.ui.indexHtml) {
    return config.ui.indexHtml;
  }
  return `<!doctype html>
<html>
  <head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <title>${config.app.name}</title>
  </head>
  <body style="background: ${config.ui.backgroundColor}; margin: 0; padding: 0">
    <div id="app"></div>
    <script>
      window.location.href = '${config.remote.url}';
    </script>
  </body>
</html>
`;
}

function writeDistIndex (config) {
  const indexPath = path.join(ROOT, 'dist', 'index.html');
  const html = getIndexHtml(config);
  fs.mkdirSync(path.dirname(indexPath), { recursive: true });
  fs.writeFileSync(indexPath, html);
  console.log('[OK] dist/index.html updated');
}

function writeAndroidStrings (config) {
  const stringsPath = path.join(ROOT, 'src-tauri', 'gen', 'android', 'app', 'src', 'main', 'res', 'values', 'strings.xml');
  const xml = `<resources>
    <string name="app_name">${config.app.name}</string>
    <string name="main_activity_title">${config.app.name}</string>
</resources>
`;
  fs.writeFileSync(stringsPath, xml);
  console.log('[OK] Android strings.xml updated');
}

function writeRustConst (config) {
  const constPath = path.join(ROOT, 'src-tauri', 'src', 'cache', 'const.rs');
  const rust = `/// Auto-generated from app.config.json — do not edit manually.

/// The remote URL to fetch the resource from.
pub const REMOTE_URL: &str = "${config.remote.url}";

/// The base URL for resolving relative resource paths.
pub const REMOTE_BASE_URL: &str = "${config.remote.baseUrl}";

/// Subdirectory under app_cache_dir for cached web resources.
pub const CACHE_DIR: &str = "${config.cache.directory}";

/// Timeout in seconds for remote fetch requests.
pub const FETCH_TIMEOUT_SECS: u64 = ${config.cache.fetchTimeoutSecs};

/// Maximum response body size in bytes.
pub const MAX_BODY_SIZE: usize = ${config.cache.maxBodySizeBytes};
`;
  fs.writeFileSync(constPath, rust);
  console.log('[OK] Rust const.rs updated');
}

function writeAndroidConfig (config) {
  const configKtPath = path.join(ROOT, 'src-tauri', 'gen', 'android', 'app', 'src', 'main', 'java', 'com', 'euv', 'AppConfig.kt');
  const criticalResources = config.remote.criticalResources
    .map(r => `        "${r}"`)
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

function main () {
  console.log('[apply-config] Reading app.config.json...');
  const config = loadConfig();
  console.log(`[apply-config] App: ${config.app.name} v${config.app.version}`);

  writeTauriConf(config);
  writeDistIndex(config);
  writeAndroidStrings(config);
  writeRustConst(config);
  writeAndroidConfig(config);

  console.log('[apply-config] Done! All platform configs updated.');
}

main();
