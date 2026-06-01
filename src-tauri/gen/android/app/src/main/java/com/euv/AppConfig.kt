package com.euv

/**
 * Auto-generated from app.config.json — do not edit manually.
 * Run: node scripts/apply-config.js
 */
object AppConfig {
    // Remote
    const val REMOTE_URL = "https://ltpp.vip/euv"
    const val REMOTE_BASE_URL = "https://ltpp.vip/static/euv/"
    val CRITICAL_SUBRESOURCES = listOf(
        "pkg/euv.js",
        "pkg/euv_bg.wasm"
    )

    // Cache
    const val CACHE_DIR = "euv_web_cache"
    const val CONNECT_TIMEOUT_FAST = 5000
    const val READ_TIMEOUT_FAST = 8000
    const val CONNECT_TIMEOUT_MISS = 15000
    const val READ_TIMEOUT_MISS = 20000
    const val MAX_REDIRECTS = 10

    // UI
    const val BACKGROUND_COLOR = "#FFFFFF"
    const val SPLASH_FADE_DURATION_MS = 300L
    const val SPLASH_MAX_WAIT_MS = 5000L
    const val IMMERSIVE_MODE = true

    // Loading HTML — shown when offline and no cache available
    val LOADING_HTML = """<!DOCTYPE html><html><head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width,initial-scale=1.0">
<title>Euv</title>
<style>body{margin:0;display:flex;align-items:center;justify-content:center;height:100vh;font-family:sans-serif;background:#FFFFFF}
@keyframes spin{0%{transform:rotate(0deg)}100%{transform:rotate(360deg)}}
.loader{width:48px;height:48px;border:5px solid #e0e0e0;border-top:5px solid #1677ff;border-radius:50%;animation:spin 0.8s linear infinite}
</style>
</head><body><div class="loader"></div></body></html>"""

    // Index HTML — the frontend entry point loaded by Tauri
    val INDEX_HTML = """<!doctype html>
<html>
  <head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <title>Euv</title>
  </head>
  <body style="background: #FFFFFF; margin: 0; padding: 0">
    <div id="app"></div>
    <script>
      window.location.href = 'https://ltpp.vip/euv';
    </script>
  </body>
</html>
"""

    // Android
    const val KEEP_ALIVE_SERVICE = true
    const val NOTIFICATION_CHANNEL_ID = "euv_keep_alive"
    const val NOTIFICATION_CHANNEL_NAME = "后台运行"
    const val NOTIFICATION_TITLE = "EUV"
    const val NOTIFICATION_TEXT = "运行中"
}
