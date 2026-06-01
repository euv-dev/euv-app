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
    // Timeouts: shorter for cache-hit background refresh, longer only for cold miss
    private val CONNECT_TIMEOUT_FAST = 5_000  // 5s for background refresh
    private val READ_TIMEOUT_FAST = 8_000     // 8s for background refresh
    private val CONNECT_TIMEOUT_MISS = 15_000 // 15s for cache miss (must wait)
    private val READ_TIMEOUT_MISS = 20_000    // 20s for cache miss
    // The base URL of the remote page (restored from cache or default)
    @Volatile private var remoteBaseUrl: String = try {
        val f = File(File(context.cacheDir, "euv_web_cache"), "base_url.txt")
        if (f.exists()) f.readText().trim() else "https://ltpp.vip/static/euv/"
    } catch (_: Exception) { "https://ltpp.vip/static/euv/" }
    // Track if background refresh already started this session (avoid duplicate refreshes)
    private val refreshedUrls = mutableSetOf<String>()
    // In-memory cache for prefetched resources (avoids disk I/O on hot path)
    private val memoryCache = java.util.concurrent.ConcurrentHashMap<String, Pair<ByteArray, String>>()
    // Known critical sub-resources to prefetch in parallel after main page cache hit
    private val CRITICAL_SUBRESOURCES = listOf("pkg/euv.js", "pkg/euv_bg.wasm")
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

        // Main frame request to tauri.localhost → serve remote page from cache (offline-first)
        if (host == "tauri.localhost" && isMainFrame && url == "http://tauri.localhost/") {
            // If main frame already loaded, this is a reload triggered by JS — block it
            if (mainFrameLoaded) {
                debugLog(">>> MAIN-FRAME BLOCKED (already loaded, preventing JS-triggered reload)")
                return WebResourceResponse(
                    "text/html", "utf-8", 200, "OK",
                    mapOf("Cache-Control" to "no-store"),
                    ByteArrayInputStream("<!-- blocked reload -->".toByteArray())
                )
            }
            debugLog(">>> MAIN-FRAME serving remote page (offline-first)")
            interceptedState[url] = true
            mainFrameLoaded = true
            // Fetch the remote page (follows redirects), cache it, serve from cache
            val mainPageResponse = serveCachedOrFetchMainPage("https://ltpp.vip/euv")
            if (mainPageResponse != null) {
                return mainPageResponse
            }
            // If both cache and network fail, serve loading page and retry in background
            debugLog(">>> MAIN-FRAME no cache and no network, serving loading + auto-retry")
            mainFrameLoaded = false  // Allow reload after retry succeeds
            // Background retry loop
            thread {
                var attempt = 0
                while (attempt < 60) { // retry up to ~60 times (~2 min)
                    attempt++
                    try { Thread.sleep(2000) } catch (_: Exception) {}
                    debugLog(">>> AUTO-RETRY attempt #$attempt")
                    val result = fetchAndStoreWithFinalUrl("https://ltpp.vip/euv")
                    if (result != null) {
                        remoteBaseUrl = result.baseUrl
                        try { File(cacheDir, "base_url.txt").writeText(result.baseUrl) } catch (_: Exception) {}
                        debugLog(">>> AUTO-RETRY SUCCESS at attempt #$attempt, reloading WebView")
                        Handler(Looper.getMainLooper()).post {
                            view.loadUrl("http://tauri.localhost/")
                        }
                        return@thread
                    }
                }
                debugLog(">>> AUTO-RETRY gave up after $attempt attempts")
            }
            val fallbackHtml = """<!DOCTYPE html><html><head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width,initial-scale=1.0">
<style>body{margin:0;display:flex;align-items:center;justify-content:center;height:100vh;font-family:sans-serif;background:#fff}
@keyframes spin{0%{transform:rotate(0deg)}100%{transform:rotate(360deg)}}
.loader{width:48px;height:48px;border:5px solid #e0e0e0;border-top:5px solid #1677ff;border-radius:50%;animation:spin 0.8s linear infinite}
</style>
</head><body><div class="loader"></div></body></html>"""
            val bytes = fallbackHtml.toByteArray(Charsets.UTF_8)
            val headers = mutableMapOf("Cache-Control" to "no-store")
            return WebResourceResponse("text/html", "utf-8", 200, "OK", headers, ByteArrayInputStream(bytes))
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

        // External http/https → offline-first caching for all requests
        if (url.startsWith("http://") || url.startsWith("https://")) {
            debugLog(">>> CACHE path for $url isMainFrame=$isMainFrame")
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
            // Return null to let WebView handle it normally (network fallback)
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
     * Main page fetch with offline-first support:
     * - If cached → serve immediately from disk + background refresh
     * - If not cached → fetch with redirect following, cache, return
     * Also resolves and stores the final base URL for relative resource paths.
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
                remoteBaseUrl = baseFile.readText().trim()
                debugLog("MAIN-HIT restored baseUrl=$remoteBaseUrl")
            }

            // Background refresh (only once per session) — use fast timeout
            if (refreshedUrls.add(url)) {
                thread(priority = Thread.MIN_PRIORITY) {
                    try {
                        val result = fetchAndStoreWithFinalUrl(url, fast = true)
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

            // Prefetch critical sub-resources into memory cache in parallel
            prefetchCriticalResources()

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
                debugLog("MAIN-FETCH-NULL $url (offline and no cache)")
                return null
            }
            remoteBaseUrl = result.baseUrl
            try { baseFile.writeText(result.baseUrl) } catch (_: Exception) {}
            debugLog("MAIN-MISS-SERVED $url baseUrl=${result.baseUrl}")
            val mimeType = extractMimeType(result.contentType)
            val encoding = extractEncoding(result.contentType) ?: "utf-8"
            val headers = mutableMapOf(
                "Access-Control-Allow-Origin" to "*",
                "Cache-Control" to "no-store"
            )
            return WebResourceResponse(mimeType, encoding, 200, "OK", headers, ByteArrayInputStream(result.data))
        }
    }

    /**
     * Offline-first: cache exists → serve immediately + background refresh.
     * No cache → fetch synchronously, store, return.
     */
    private fun serveCachedOrFetch(url: String): WebResourceResponse? {
        // 1. Check memory cache first (fastest path — prefetched resources)
        memoryCache[url]?.let { (data, contentType) ->
            debugLog("MEM-HIT $url (${data.size}B)")
            val mimeType = extractMimeType(contentType)
            val encoding = if (isBinaryMime(mimeType)) null else (extractEncoding(contentType) ?: "utf-8")
            val headers = mutableMapOf(
                "Access-Control-Allow-Origin" to "*",
                "Cache-Control" to "no-store"
            )
            // Background refresh (only once per session)
            if (refreshedUrls.add(url)) {
                thread(priority = Thread.MIN_PRIORITY) {
                    try { fetchAndStore(url, fast = true); debugLog("BG-OK $url") }
                    catch (e: Exception) { debugLog("BG-FAIL $url: ${e.message}") }
                }
            }
            return WebResourceResponse(mimeType, encoding, 200, "OK", headers, ByteArrayInputStream(data))
        }

        // 2. Check disk cache
        val dataFile = getCacheFile(url)
        val metaFile = getMetaFile(url)

        if (dataFile.exists() && metaFile.exists() && dataFile.length() > 0) {
            debugLog("HIT $url (${dataFile.length()}B)")
            val contentType = metaFile.readText()
            val mimeType = extractMimeType(contentType)
            val encoding = if (isBinaryMime(mimeType)) null else (extractEncoding(contentType) ?: "utf-8")

            // Background refresh (only once per session) — use fast timeout, low priority
            if (refreshedUrls.add(url)) {
                thread(priority = Thread.MIN_PRIORITY) {
                    try {
                        fetchAndStore(url, fast = true)
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
            // Also store in memory cache for potential subsequent requests
            memoryCache[url] = Pair(result.data, result.contentType)
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

    private fun fetchAndStoreWithFinalUrl(originalUrl: String, fast: Boolean = false): FetchResultWithUrl? {
        var cur = originalUrl
        var redir = 0
        val connTimeout = if (fast) CONNECT_TIMEOUT_FAST else CONNECT_TIMEOUT_MISS
        val readTimeout = if (fast) READ_TIMEOUT_FAST else READ_TIMEOUT_MISS
        while (redir < MAX_REDIRECTS) {
            debugLog("MAIN-FETCH $cur (redirect #$redir, fast=$fast)")
            val conn = URL(cur).openConnection() as HttpURLConnection
            conn.instanceFollowRedirects = false
            conn.connectTimeout = connTimeout
            conn.readTimeout = readTimeout
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

    private fun fetchAndStore(originalUrl: String, fast: Boolean = false): FetchResult? {
        var cur = originalUrl
        var redir = 0
        val connTimeout = if (fast) CONNECT_TIMEOUT_FAST else CONNECT_TIMEOUT_MISS
        val readTimeout = if (fast) READ_TIMEOUT_FAST else READ_TIMEOUT_MISS
        while (redir < MAX_REDIRECTS) {
            debugLog("FETCH $cur (redirect #$redir, fast=$fast)")
            val conn = URL(cur).openConnection() as HttpURLConnection
            conn.instanceFollowRedirects = false
            conn.connectTimeout = connTimeout
            conn.readTimeout = readTimeout
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
     * Prefetch critical sub-resources (JS + WASM) into memory cache in parallel.
     * Called immediately after main page cache hit, before WebView starts parsing HTML.
     * This ensures sub-resources are ready when WebView requests them.
     */
    private fun prefetchCriticalResources() {
        val base = remoteBaseUrl
        debugLog("PREFETCH starting for ${CRITICAL_SUBRESOURCES.size} resources, base=$base")
        for (subPath in CRITICAL_SUBRESOURCES) {
            val fullUrl = base + subPath
            // Skip if already in memory cache
            if (memoryCache.containsKey(fullUrl)) {
                debugLog("PREFETCH skip (memory hit): $fullUrl")
                continue
            }
            thread {
                try {
                    val dataFile = getCacheFile(fullUrl)
                    val metaFile = getMetaFile(fullUrl)
                    if (dataFile.exists() && metaFile.exists() && dataFile.length() > 0) {
                        // Load from disk into memory cache for instant serving
                        val data = dataFile.readBytes()
                        val ct = metaFile.readText()
                        memoryCache[fullUrl] = Pair(data, ct)
                        debugLog("PREFETCH disk→mem OK: $fullUrl (${data.size}B)")
                    } else {
                        // Fetch from network and store both disk + memory
                        val result = fetchAndStore(fullUrl)
                        if (result != null) {
                            memoryCache[fullUrl] = Pair(result.data, result.contentType)
                            debugLog("PREFETCH net→mem OK: $fullUrl (${result.data.size}B)")
                        }
                    }
                } catch (e: Exception) {
                    debugLog("PREFETCH ERR: $fullUrl: ${e.message}")
                }
            }
        }
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

        // Once the main frame has loaded (redirected from tauri.localhost),
        // allow all HTTP/HTTPS navigation within the WebView
        if (mainFrameLoaded && (url.startsWith("http://") || url.startsWith("https://"))) {
            debugLog(">>> Allowing in-WebView navigation to $url")
            return false
        }

        // Before main frame loads, open external links in browser
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
        // The main page is served at tauri.localhost but contains remote content.
        // Once it finishes loading, inject a check for WASM app render, then remove splash.
        if (url == "http://tauri.localhost/" && mainFrameLoaded) {
            debugLog(">>> Main page finished, waiting for app render to remove splash")
            // Poll for the #app div having children (WASM app rendered)
            // Use aggressive polling: start immediately, 50ms interval (was 200ms + 100ms)
            val handler = Handler(Looper.getMainLooper())
            val pollRunnable = object : Runnable {
                var attempts = 0
                override fun run() {
                    attempts++
                    view.evaluateJavascript(
                        "(document.getElementById('app') && document.getElementById('app').children.length > 0) || document.body.children.length > 1"
                    ) { result ->
                        if (result == "true" || attempts > 60) {
                            debugLog(">>> App rendered (attempts=$attempts), removing splash")
                            (context as? MainActivity)?.removeSplash()
                        } else {
                            handler.postDelayed(this, 50)
                        }
                    }
                }
            }
            // Start polling immediately (no initial delay)
            handler.post(pollRunnable)
            // Fallback: remove splash after 5s max (was 8s)
            handler.postDelayed({
                (context as? MainActivity)?.removeSplash()
            }, 5000)
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
