/* THIS FILE IS AUTO-GENERATED. DO NOT MODIFY!! */

// Copyright 2020-2023 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

package com.euv

import android.content.Intent
import android.net.Uri
import android.webkit.*
import android.content.Context
import android.graphics.Bitmap
import android.os.Handler
import android.os.Looper
import android.util.Log
import androidx.webkit.WebViewAssetLoader
import java.io.ByteArrayInputStream
import java.io.BufferedInputStream
import java.io.File
import java.io.FileInputStream
import java.io.FileOutputStream
import java.net.HttpURLConnection
import java.net.URL
import java.security.MessageDigest
import java.util.zip.GZIPInputStream
import kotlin.concurrent.thread

class RustWebViewClient(webView: RustWebView, private val context: Context): WebViewClient() {
    private val interceptedState = mutableMapOf<String, Boolean>()
    var currentUrl: String = "about:blank"
    private var lastInterceptedUrl: Uri? = null
    private var pendingUrlRedirect: String? = null

    private val assetLoader = WebViewAssetLoader.Builder()
        .setDomain(Rust.assetLoaderDomain(webView.id))
        .addPathHandler("/", WebViewAssetLoader.AssetsPathHandler(context))
        .build()

    // ===== Offline cache =====
    private val TAG = "EUV_CACHE"
    private val cacheDir = File(context.cacheDir, "euv_web_cache").also { it.mkdirs() }
    private val debugFile = File(context.cacheDir, "euv_debug.log")
    private val MAX_REDIRECTS = 10
    private val assetDomain: String = Rust.assetLoaderDomain(webView.id)
    private val webViewId: String = webView.id
    // The base URL of the remote page (restored from cache or default)
    @Volatile private var remoteBaseUrl: String = try {
        val f = File(File(context.cacheDir, "euv_web_cache"), "base_url.txt")
        if (f.exists()) f.readText().trim() else "https://ltpp.vip/static/euv/"
    } catch (_: Exception) { "https://ltpp.vip/static/euv/" }
    // Track if background refresh already started this session (avoid duplicate refreshes)
    private val refreshedUrls = mutableSetOf<String>()
    // Track if main frame has been loaded (prevent WASM app from triggering reload)
    // Use companion object so it persists across WebViewClient recreations
    companion object {
        @Volatile private var mainFrameLoaded = false

        /** Reset state when Activity is (re-)created so cold/warm starts work correctly. */
        fun resetMainFrameState() {
            mainFrameLoaded = false
        }
    }

    private fun debugLog(msg: String) {
        try {
            Log.e(TAG, msg)
            debugFile.appendText("${System.currentTimeMillis()} $msg\n")
        } catch (_: Exception) {}
    }

    override fun shouldInterceptRequest(
        view: WebView,
        request: WebResourceRequest
    ): WebResourceResponse? {
        val url = request.url.toString()
        val isMainFrame = request.isForMainFrame
        val host = request.url.host ?: ""
        val useAssetLoader = Rust.withAssetLoader(webViewId)

        debugLog(">>> shouldInterceptRequest url=$url isMainFrame=$isMainFrame host=$host useAssetLoader=$useAssetLoader assetDomain=$assetDomain")

        pendingUrlRedirect?.let {
            Handler(Looper.getMainLooper()).post {
              view.loadUrl(it)
            }
            pendingUrlRedirect = null
            return null
        }

        lastInterceptedUrl = request.url

        // Asset loader domain → use Tauri asset loader
        if (useAssetLoader && host == assetDomain) {
            debugLog(">>> ASSET-LOADER path")
            return assetLoader.shouldInterceptRequest(request.url)
        }

        // Main frame request to tauri.localhost → serve optimized bootstrap HTML directly
        if (host == "tauri.localhost" && isMainFrame && url == "http://tauri.localhost/") {
            // If main frame already loaded, this is a reload triggered by WASM app — block it
            if (mainFrameLoaded) {
                debugLog(">>> MAIN-FRAME BLOCKED (already loaded, preventing WASM-triggered reload)")
                return WebResourceResponse(
                    "text/html", "utf-8", 200, "OK",
                    mapOf("Cache-Control" to "no-store"),
                    ByteArrayInputStream("<!-- blocked reload -->".toByteArray())
                )
            }
            debugLog(">>> MAIN-FRAME serving optimized bootstrap HTML")
            interceptedState[url] = true
            mainFrameLoaded = true
            // Trigger background refresh of remote resources
            if (refreshedUrls.add("https://ltpp.vip/euv")) {
                thread {
                    try {
                        val result = fetchAndStoreWithFinalUrl("https://ltpp.vip/euv")
                        if (result != null) {
                            remoteBaseUrl = result.baseUrl
                            File(cacheDir, "base_url.txt").writeText(result.baseUrl)
                            debugLog("MAIN-BG-OK baseUrl=${result.baseUrl}")
                        }
                    } catch (e: Exception) {
                        debugLog("MAIN-BG-FAIL: ${e.message}")
                    }
                }
            }
            // Return optimized HTML immediately — no disk I/O, no cache lookup needed
            return generateOptimizedHtml()
        }

        // Tauri internal domains: sub-resources → map to remote URL and cache
        if (host == "tauri.localhost" && !isMainFrame) {
            val path = request.url.path ?: ""
            if (path.isNotEmpty() && path != "/" && path != "/favicon.ico") {
                // Map relative path to remote base URL
                val remoteUrl = remoteBaseUrl + path.trimStart('/')
                debugLog(">>> TAURI-SUB-RESOURCE $path → $remoteUrl")
                try {
                    val cached = serveCachedOrFetch(remoteUrl)
                    if (cached != null) {
                        debugLog(">>> TAURI-SUB-RESOURCE served: $remoteUrl")
                        interceptedState[url] = true
                        return cached
                    }
                    debugLog(">>> TAURI-SUB-RESOURCE cache null: $remoteUrl")
                } catch (e: Exception) {
                    debugLog(">>> TAURI-SUB-RESOURCE exception: ${e.message}")
                }
            }
            // For favicon or failed sub-resources, fall through to Rust handler
            debugLog(">>> TAURI-INTERNAL fallback for $url")
            val wv = view as RustWebView
            val response = Rust.handleRequest(wv.id, request, wv.isDocumentStartScriptEnabled)
            interceptedState[url] = response != null
            return response
        }

        // Other Tauri internal domains → use Rust.handleRequest()
        if (host == assetDomain || host.endsWith(".localhost")) {
            debugLog(">>> TAURI-INTERNAL path for $url")
            val wv = view as RustWebView
            val response = Rust.handleRequest(wv.id, request, wv.isDocumentStartScriptEnabled)
            interceptedState[url] = response != null
            return response
        }

        // External http/https → offline-first caching
        if (url.startsWith("http://") || url.startsWith("https://")) {
            debugLog(">>> CACHE path for $url")
            try {
                val cached = serveCachedOrFetch(url)
                if (cached != null) {
                    debugLog(">>> CACHE returned response for $url")
                    interceptedState[url] = true
                    return cached
                }
                debugLog(">>> CACHE returned null for $url, falling through")
            } catch (e: Exception) {
                debugLog(">>> CACHE exception for $url: ${e.message}")
            }
            // Return null to let WebView handle it normally
            return null
        }

        // Fallback to Tauri default
        val wv = view as RustWebView
        val response = Rust.handleRequest(wv.id, request, wv.isDocumentStartScriptEnabled)
        if (response != null) {
            if (response.responseHeaders != null) {
                response.responseHeaders["Cache-Control"] = "no-store"
            } else {
                response.responseHeaders = mapOf("Cache-Control" to "no-store")
            }
        }
        interceptedState[url] = response != null
        return response
    }

    /**
     * Main page fetch: similar to serveCachedOrFetch but also resolves redirects
     * to determine the base URL for relative resource paths.
     */
    private fun serveCachedOrFetchMainPage(url: String): WebResourceResponse? {
        val dataFile = getCacheFile(url)
        val metaFile = getMetaFile(url)
        val baseFile = File(cacheDir, "base_url.txt")

        if (dataFile.exists() && metaFile.exists() && dataFile.length() > 0) {
            debugLog("MAIN-HIT $url (${dataFile.length()}B)")
            val contentType = metaFile.readText()
            val mimeType = extractMimeType(contentType)
            val encoding = extractEncoding(contentType) ?: "utf-8"
            // Restore base URL from saved file
            if (baseFile.exists()) {
                remoteBaseUrl = baseFile.readText()
                debugLog("MAIN-HIT restored baseUrl=$remoteBaseUrl")
            }

            // Background refresh (only once per session)
            if (refreshedUrls.add(url)) {
                thread {
                    try {
                        val result = fetchAndStoreWithFinalUrl(url)
                        if (result != null) {
                            remoteBaseUrl = result.baseUrl
                            baseFile.writeText(result.baseUrl)
                            debugLog("MAIN-BG-OK baseUrl=${result.baseUrl}")
                        }
                    } catch (e: Exception) {
                        debugLog("MAIN-BG-FAIL: ${e.message}")
                    }
                }
            }

            // Stream directly from file
            val inputStream = BufferedInputStream(FileInputStream(dataFile), 65536)
            val headers = mutableMapOf(
                "Access-Control-Allow-Origin" to "*",
                "Cache-Control" to "no-store"
            )
            return WebResourceResponse(mimeType, encoding, 200, "OK", headers, inputStream)
        } else {
            debugLog("MAIN-MISS $url")
            val result = fetchAndStoreWithFinalUrl(url)
            if (result == null) {
                debugLog("MAIN-FETCH-NULL $url")
                return null
            }
            remoteBaseUrl = result.baseUrl
            try { baseFile.writeText(result.baseUrl) } catch (_: Exception) {}
            debugLog("MAIN-MISS-SERVED $url baseUrl=${result.baseUrl}")
            val mimeType = extractMimeType(result.contentType)
            val encoding = extractEncoding(result.contentType)
            val headers = mutableMapOf(
                "Access-Control-Allow-Origin" to "*",
                "Cache-Control" to "no-store"
            )
            return WebResourceResponse(mimeType, encoding ?: "utf-8", 200, "OK", headers, ByteArrayInputStream(result.data))
        }
    }

    /**
     * Offline-first: cache exists → serve immediately + background refresh.
     * No cache → fetch synchronously, store, return.
     */
    private fun serveCachedOrFetch(url: String): WebResourceResponse? {
        val dataFile = getCacheFile(url)
        val metaFile = getMetaFile(url)

        if (dataFile.exists() && metaFile.exists() && dataFile.length() > 0) {
            debugLog("HIT $url (${dataFile.length()}B)")
            val contentType = metaFile.readText()
            val mimeType = extractMimeType(contentType)
            val encoding = if (isBinaryMime(mimeType)) null else (extractEncoding(contentType) ?: "utf-8")

            // Background refresh (only once per session)
            if (refreshedUrls.add(url)) {
                thread {
                    try {
                        fetchAndStore(url)
                        debugLog("BG-OK $url")
                    } catch (e: Exception) {
                        debugLog("BG-FAIL $url: ${e.message}")
                    }
                }
            }

            // Stream directly from file — avoids loading entire file into memory
            val inputStream = BufferedInputStream(FileInputStream(dataFile), 65536)
            val headers = mutableMapOf(
                "Access-Control-Allow-Origin" to "*",
                "Cache-Control" to "no-store"
            )
            return WebResourceResponse(mimeType, encoding, 200, "OK", headers, inputStream)
        } else {
            debugLog("MISS $url")
            val result = fetchAndStore(url)
            if (result == null) {
                debugLog("FETCH-RETURNED-NULL $url")
                return null
            }
            val mimeType = extractMimeType(result.contentType)
            val encoding = if (isBinaryMime(mimeType)) null else (extractEncoding(result.contentType) ?: "utf-8")
            val headers = mutableMapOf(
                "Access-Control-Allow-Origin" to "*",
                "Cache-Control" to "no-store"
            )
            debugLog("MISS-SERVED $url (${result.data.size}B, $mimeType)")
            return WebResourceResponse(mimeType, encoding, 200, "OK", headers, ByteArrayInputStream(result.data))
        }
    }

    private data class FetchResultWithUrl(val data: ByteArray, val contentType: String, val baseUrl: String)

    private fun fetchAndStoreWithFinalUrl(originalUrl: String): FetchResultWithUrl? {
        var cur = originalUrl
        var redir = 0
        while (redir < MAX_REDIRECTS) {
            debugLog("MAIN-FETCH $cur (redirect #$redir)")
            val conn = URL(cur).openConnection() as HttpURLConnection
            conn.instanceFollowRedirects = false
            conn.connectTimeout = 30_000
            conn.readTimeout = 30_000
            conn.setRequestProperty("Accept-Encoding", "gzip, deflate")
            conn.setRequestProperty("User-Agent",
                "Mozilla/5.0 (Linux; Android) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Mobile Safari/537.36")
            try {
                conn.connect()
                val code = conn.responseCode
                debugLog("MAIN-FETCH-RESP $code for $cur")
                if (code in listOf(301, 302, 303, 307, 308)) {
                    val loc = conn.getHeaderField("Location")
                    conn.disconnect()
                    if (loc != null) { cur = URL(URL(cur), loc).toString(); redir++; continue }
                    return null
                }
                if (code != 200) { conn.disconnect(); return null }

                val ct = conn.contentType ?: "application/octet-stream"
                val ce = conn.getHeaderField("Content-Encoding")
                val data = if (ce?.contains("gzip", true) == true) {
                    GZIPInputStream(conn.inputStream).use { it.readBytes() }
                } else {
                    conn.inputStream.use { it.readBytes() }
                }
                conn.disconnect()

                writeCache(cur, data, ct)
                if (cur != originalUrl) writeCache(originalUrl, data, ct)

                // Compute base URL (directory of the final URL)
                val baseUrl = if (cur.contains("/")) {
                    cur.substringBeforeLast("/") + "/"
                } else {
                    cur + "/"
                }
                debugLog("MAIN-STORED ${data.size}B baseUrl=$baseUrl")
                return FetchResultWithUrl(data, ct, baseUrl)
            } catch (e: Exception) {
                conn.disconnect()
                debugLog("MAIN-NET-ERR $cur: ${e.message}")
                return null
            }
        }
        return null
    }

    private fun fetchAndStore(originalUrl: String): FetchResult? {
        var cur = originalUrl
        var redir = 0
        while (redir < MAX_REDIRECTS) {
            debugLog("FETCH $cur (redirect #$redir)")
            val conn = URL(cur).openConnection() as HttpURLConnection
            conn.instanceFollowRedirects = false
            conn.connectTimeout = 30_000
            conn.readTimeout = 30_000
            conn.setRequestProperty("Accept-Encoding", "gzip, deflate")
            conn.setRequestProperty("User-Agent",
                "Mozilla/5.0 (Linux; Android) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Mobile Safari/537.36")
            try {
                conn.connect()
                val code = conn.responseCode
                debugLog("FETCH-RESP $code for $cur")
                if (code in listOf(301, 302, 303, 307, 308)) {
                    val loc = conn.getHeaderField("Location")
                    conn.disconnect()
                    if (loc != null) { cur = URL(URL(cur), loc).toString(); redir++; continue }
                    return null
                }
                if (code != 200) { conn.disconnect(); return null }

                val ct = conn.contentType ?: "application/octet-stream"
                val ce = conn.getHeaderField("Content-Encoding")
                val data = if (ce?.contains("gzip", true) == true) {
                    GZIPInputStream(conn.inputStream).use { it.readBytes() }
                } else {
                    conn.inputStream.use { it.readBytes() }
                }
                conn.disconnect()

                writeCache(cur, data, ct)
                if (cur != originalUrl) writeCache(originalUrl, data, ct)
                debugLog("STORED ${data.size}B $cur")
                return FetchResult(data, ct)
            } catch (e: Exception) {
                conn.disconnect()
                debugLog("NET-ERR $cur: ${e.message}")
                return null
            }
        }
        return null
    }

    private fun writeCache(url: String, data: ByteArray, ct: String) {
        try {
            val df = getCacheFile(url); val mf = getMetaFile(url)
            df.parentFile?.mkdirs()
            FileOutputStream(df).use { it.write(data) }
            FileOutputStream(mf).use { it.write(ct.toByteArray()) }
        } catch (e: Exception) {
            debugLog("WRITE-ERR $url: ${e.message}")
        }
    }

    /**
     * Generate an optimized bootstrap HTML that:
     * 1. Starts WASM fetch + compile immediately (no waiting for module script parsing)
     * 2. Loads JS in parallel
     * 3. Uses instantiateStreaming for fastest possible WASM compilation
     * 4. Shows no loading text, just a spinner
     *
     * This completely replaces the remote HTML with a faster-loading equivalent.
     */
    private fun generateOptimizedHtml(): WebResourceResponse {
        // No CSS animation — native splash handles the visual feedback.
        // WASM module is cached in IndexedDB after first compile, so subsequent
        // launches skip the expensive ~500ms compilation entirely.
        val html = """<!DOCTYPE html><html><head>
<meta name="viewport" content="width=device-width,initial-scale=1.0,maximum-scale=1.0,user-scalable=no">
<title>Euv</title>
<style>body{margin:0;padding:0;background:#fff}</style>
</head><body>
<div id="app"></div>
<script>
(function(){
// IndexedDB cache for compiled WASM module — skips ~500ms recompilation
var DB='euv_wasm',ST='m',K='w';
function openDB(){return new Promise(function(r){var q=indexedDB.open(DB,1);q.onupgradeneeded=function(e){e.target.result.createObjectStore(ST)};q.onsuccess=function(e){r(e.target.result)};q.onerror=function(){r(null)}})}
function getM(db){if(!db)return Promise.resolve(null);return new Promise(function(r){try{var t=db.transaction(ST);var q=t.objectStore(ST).get(K);q.onsuccess=function(){r(q.result||null)};q.onerror=function(){r(null)}}catch(e){r(null)}})}
function putM(db,m){if(!db)return;try{db.transaction(ST,'readwrite').objectStore(ST).put(m,K)}catch(e){}}

// Start JS module load immediately (parallel)
var js=import('./pkg/euv.js');

openDB().then(function(db){
  return getM(db).then(function(cached){
    if(cached){
      // Cache hit: pass compiled Module directly to wasm-bindgen init (~5ms)
      return js.then(function(m){return m.default(cached).then(function(){m.main()})});
    }
    // Cache miss: compile + store, then init
    return WebAssembly.compileStreaming(fetch('./pkg/euv_bg.wasm')).then(function(mod){
      putM(db,mod);
      return js.then(function(m){return m.default(mod).then(function(){m.main()})});
    });
  });
}).catch(function(){
  // Fallback without cache
  js.then(function(m){return m.default().then(function(){m.main()})});
});
})();
</script>
</body></html>"""

        val bytes = html.toByteArray(Charsets.UTF_8)
        val headers = mutableMapOf(
            "Access-Control-Allow-Origin" to "*",
            "Cache-Control" to "no-store"
        )
        return WebResourceResponse("text/html", "utf-8", 200, "OK", headers, ByteArrayInputStream(bytes))
    }

    private fun getCacheFile(url: String) = File(cacheDir, "${sha256(url)}${extOf(url)}")
    private fun getMetaFile(url: String) = File(cacheDir, "${sha256(url)}.meta")
    private fun extractMimeType(ct: String) = ct.split(";").firstOrNull()?.trim() ?: "application/octet-stream"
    private fun extractEncoding(ct: String): String? = ct.split(";").find { it.trim().startsWith("charset=", true) }?.substringAfter("=")?.trim()
    private fun isBinaryMime(mime: String): Boolean = mime.startsWith("application/wasm") || mime.startsWith("application/octet") || mime.startsWith("image/") || mime.startsWith("audio/") || mime.startsWith("video/")
    private fun extOf(url: String): String {
        return try { val p = URL(url).path; val d = p.lastIndexOf('.'); val s = p.lastIndexOf('/')
            if (d > s && d < p.length - 1) { val e = p.substring(d); if (e.length <= 10) e else "" } else ""
        } catch (_: Exception) { "" }
    }
    private fun sha256(s: String): String { val d = MessageDigest.getInstance("SHA-256").digest(s.toByteArray()); return d.joinToString("") { "%02x".format(it) } }
    private data class FetchResult(val data: ByteArray, val contentType: String)

    // ===== Original Tauri callbacks =====

    override fun shouldOverrideUrlLoading(
        view: WebView,
        request: WebResourceRequest
    ): Boolean {
        val url = request.url.toString()
        val host = request.url.host ?: ""
        debugLog(">>> shouldOverrideUrlLoading url=$url host=$host")

        // Allow tauri internal navigation
        if (host == "tauri.localhost" || url.startsWith("tauri://")) {
            val result = Rust.shouldOverride((view as RustWebView).id, url)
            return result
        }

        // All other links open in external browser
        try {
            val intent = Intent(Intent.ACTION_VIEW, Uri.parse(url))
            intent.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
            context.startActivity(intent)
            debugLog(">>> Opened in external browser: $url")
        } catch (e: Exception) {
            debugLog(">>> Failed to open external browser: ${e.message}")
        }
        return true  // Prevent WebView from loading the URL
    }

    override fun onPageStarted(view: WebView, url: String, favicon: Bitmap?) {
        debugLog(">>> onPageStarted url=$url")
        currentUrl = url
        if (interceptedState[url] == false) {
            val webView = view as RustWebView
            for (script in webView.initScripts) {
                view.evaluateJavascript(script, null)
            }
        }
        return Rust.onPageLoading((view as RustWebView).id, url)
    }

    override fun onPageFinished(view: WebView, url: String) {
        debugLog(">>> onPageFinished url=$url")
        Rust.onPageLoaded((view as RustWebView).id, url)
        // Inject a script that waits for WASM app to render, then removes native splash
        if (url == "http://tauri.localhost/") {
            view.evaluateJavascript("""
                (function() {
                    function checkReady() {
                        var app = document.getElementById('app');
                        if (app && app.children.length > 0) {
                            window.__SPLASH_READY = true;
                            return;
                        }
                        new MutationObserver(function(m, o) {
                            var app = document.getElementById('app');
                            if (app && app.children.length > 0) {
                                window.__SPLASH_READY = true;
                                o.disconnect();
                            }
                        }).observe(document.body || document.documentElement, {childList: true, subtree: true});
                    }
                    checkReady();
                })();
            """.trimIndent(), null)
            // Poll for WASM render completion to remove native splash — start immediately
            val handler = Handler(Looper.getMainLooper())
            val pollRunnable = object : Runnable {
                override fun run() {
                    view.evaluateJavascript("window.__SPLASH_READY === true") { result ->
                        if (result == "true") {
                            debugLog(">>> WASM rendered, removing native splash")
                            (context as? MainActivity)?.removeSplash()
                        } else {
                            handler.postDelayed(this, 50)
                        }
                    }
                }
            }
            handler.postDelayed(pollRunnable, 100)
            // Fallback: remove splash after 10s max
            handler.postDelayed({
                debugLog(">>> Splash timeout, force removing")
                (context as? MainActivity)?.removeSplash()
            }, 10000)
        }
    }

    override fun onReceivedError(
        view: WebView,
        request: WebResourceRequest,
        error: WebResourceError
    ) {
        debugLog(">>> onReceivedError code=${error.errorCode} desc=${error.description} url=${request.url}")
        if (error.errorCode == ERROR_CONNECT && request.isForMainFrame && request.url != lastInterceptedUrl) {
            view.stopLoading()
            view.loadUrl(request.url.toString())
            pendingUrlRedirect = request.url.toString()
        } else {
            super.onReceivedError(view, request, error)
        }
    }

    
}
