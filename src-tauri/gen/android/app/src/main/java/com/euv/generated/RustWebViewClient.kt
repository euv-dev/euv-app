/* THIS FILE IS AUTO-GENERATED. DO NOT MODIFY!! */

// Copyright 2020-2023 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

package com.euv

import android.net.Uri
import android.webkit.*
import android.content.Context
import android.graphics.Bitmap
import android.os.Handler
import android.os.Looper
import android.util.Log
import androidx.webkit.WebViewAssetLoader
import java.io.ByteArrayInputStream
import java.io.File
import java.io.FileOutputStream
import java.net.HttpURLConnection
import java.net.URL
import java.security.MessageDigest
import java.util.zip.GZIPInputStream
import kotlin.concurrent.thread

class RustWebViewClient(webView: RustWebView, context: Context): WebViewClient() {
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
    // The base URL of the remote page (set after first fetch resolves redirects)
    @Volatile private var remoteBaseUrl: String = "https://ltpp.vip/static/euv/"

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

        // Main frame request to tauri.localhost → serve remote page content via cache
        if (host == "tauri.localhost" && isMainFrame && url == "http://tauri.localhost/") {
            debugLog(">>> MAIN-FRAME tauri.localhost → fetching remote page via cache")
            try {
                val remoteUrl = "https://ltpp.vip/euv"
                val cached = serveCachedOrFetchMainPage(remoteUrl)
                if (cached != null) {
                    debugLog(">>> MAIN-FRAME served from cache/network, remoteBaseUrl=$remoteBaseUrl")
                    interceptedState[url] = true
                    return cached
                }
                debugLog(">>> MAIN-FRAME cache returned null, falling back to Rust")
            } catch (e: Exception) {
                debugLog(">>> MAIN-FRAME exception: ${e.message}")
            }
            // Fall through to Rust handler if cache fails
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
            val encoding = extractEncoding(contentType)
            // Restore base URL from saved file
            if (baseFile.exists()) {
                remoteBaseUrl = baseFile.readText()
                debugLog("MAIN-HIT restored baseUrl=$remoteBaseUrl")
            }

            // Background refresh
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

            val bytes = dataFile.readBytes()
            val headers = mutableMapOf(
                "Access-Control-Allow-Origin" to "*",
                "Cache-Control" to "no-store"
            )
            return WebResourceResponse(mimeType, encoding ?: "utf-8", 200, "OK", headers, ByteArrayInputStream(bytes))
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
            val encoding = extractEncoding(contentType)

            // Background refresh
            thread {
                try {
                    fetchAndStore(url)
                    debugLog("BG-OK $url")
                } catch (e: Exception) {
                    debugLog("BG-FAIL $url: ${e.message}")
                }
            }

            val bytes = dataFile.readBytes()
            val headers = mutableMapOf(
                "Access-Control-Allow-Origin" to "*",
                "Cache-Control" to "no-store"
            )
            return WebResourceResponse(mimeType, encoding ?: "utf-8", 200, "OK", headers, ByteArrayInputStream(bytes))
        } else {
            debugLog("MISS $url")
            val result = fetchAndStore(url)
            if (result == null) {
                debugLog("FETCH-RETURNED-NULL $url")
                return null
            }
            val mimeType = extractMimeType(result.contentType)
            val encoding = extractEncoding(result.contentType)
            val headers = mutableMapOf(
                "Access-Control-Allow-Origin" to "*",
                "Cache-Control" to "no-store"
            )
            debugLog("MISS-SERVED $url (${result.data.size}B, $mimeType)")
            return WebResourceResponse(mimeType, encoding ?: "utf-8", 200, "OK", headers, ByteArrayInputStream(result.data))
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

    private fun getCacheFile(url: String) = File(cacheDir, "${sha256(url)}${extOf(url)}")
    private fun getMetaFile(url: String) = File(cacheDir, "${sha256(url)}.meta")
    private fun extractMimeType(ct: String) = ct.split(";").firstOrNull()?.trim() ?: "application/octet-stream"
    private fun extractEncoding(ct: String): String? = ct.split(";").find { it.trim().startsWith("charset=", true) }?.substringAfter("=")?.trim()
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
        val result = Rust.shouldOverride((view as RustWebView).id, url)
        debugLog(">>> shouldOverrideUrlLoading url=$url result=$result")
        return result
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
