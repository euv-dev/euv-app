# EUV APK Build Script
# Full build: Rust compile → Kotlin gen → Cache injection → Package → Sign

$ErrorActionPreference = "Stop"

$env:ANDROID_HOME = "$env:LOCALAPPDATA\Android\Sdk"
$env:ANDROID_SDK_ROOT = "$env:LOCALAPPDATA\Android\Sdk"
$env:NDK_HOME = "$env:LOCALAPPDATA\Android\Sdk\ndk\android-ndk-r27b"

$desktop = [Environment]::GetFolderPath("Desktop")
$projectDir = "D:\code\euv-app"
$androidDir = "$projectDir\src-tauri\gen\android"
$keystore = "$projectDir\keystore.jks"

# ===== Step 1: Full build =====
Write-Output "=== Step 1: tauri android build --ci ==="
cd $projectDir
npx @tauri-apps/cli@latest android build --ci

# ===== Step 2: Inject cache logic into RustWebViewClient.kt =====
Write-Output ""
Write-Output "=== Step 2: Inject HTTP cache into RustWebViewClient.kt ==="

$cacheKotlin = @'
/* THIS FILE IS AUTO-GENERATED. DO NOT MODIFY!! */

// Copyright 2020-2023 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

package com.euv

import android.net.Uri
import android.util.Log
import android.webkit.*
import android.content.Context
import android.graphics.Bitmap
import android.os.Handler
import android.os.Looper
import androidx.webkit.WebViewAssetLoader
import java.io.ByteArrayInputStream
import java.io.File
import java.io.FileInputStream
import java.net.HttpURLConnection
import java.net.URL
import java.util.zip.GZIPInputStream

class RustWebViewClient(webView: RustWebView, context: Context): WebViewClient() {
    private val interceptedState = mutableMapOf<String, Boolean>()
    var currentUrl: String = "about:blank"
    private var lastInterceptedUrl: Uri? = null
    private var pendingUrlRedirect: String? = null

    private val assetLoader = WebViewAssetLoader.Builder()
        .setDomain(Rust.assetLoaderDomain(webView.id))
        .addPathHandler("/", WebViewAssetLoader.AssetsPathHandler(context))
        .build()

    private val cacheDir: File = File(context.cacheDir, "euv_http_cache").apply {
        if (!exists()) mkdirs()
    }

    // The remote URL prefix we want to cache
    private val REMOTE_HOST = "ltpp.vip"
    private val REMOTE_PATH_PREFIX = "/euv"
    private val REMOTE_URL = "https://$REMOTE_HOST$REMOTE_PATH_PREFIX"
    // And run: adb reverse tcp:8888 tcp:8888

    private fun urlToCacheFile(url: String): File {
        val uri = Uri.parse(url)
        val host = uri.host ?: "unknown"
        val path = uri.path ?: "/"
        val safeHost = sanitizeDirName(host)
        val parentDir = if (path == "/" || path.isEmpty()) {
            File(cacheDir, safeHost)
        } else {
            val pathWithoutLeadingSlash = path.removePrefix("/")
            val lastSep = pathWithoutLeadingSlash.lastIndexOf('/')
            if (lastSep < 0) {
                File(cacheDir, safeHost)
            } else {
                val dirPart = pathWithoutLeadingSlash.substring(0, lastSep)
                File(cacheDir, "$safeHost/$dirPart")
            }
        }
        val filename = if (path == "/" || path.isEmpty()) {
            "index.html"
        } else {
            val pathWithoutLeadingSlash = path.removePrefix("/")
            val lastSep = pathWithoutLeadingSlash.lastIndexOf('/')
            val name = if (lastSep < 0) pathWithoutLeadingSlash else pathWithoutLeadingSlash.substring(lastSep + 1)
            if (name.isEmpty()) "index.html" else sanitizeFileName(name)
        }
        return File(parentDir, filename)
    }

    private fun getMetaFile(dataFile: File): File {
        return File(dataFile.parentFile, dataFile.name + ".meta")
    }

    private fun sanitizeDirName(name: String): String {
        return name.replace(Regex("[^a-zA-Z0-9._\\-]"), "_")
    }

    private fun sanitizeFileName(name: String): String {
        return name.replace(Regex("[^a-zA-Z0-9._\\-]"), "_")
    }

    private fun readFromCache(url: String): WebResourceResponse? {
        try {
            val dataFile = urlToCacheFile(url)
            if (!dataFile.exists() || dataFile.length() == 0L) return null
            val metaFile = getMetaFile(dataFile)
            val contentType = if (metaFile.exists()) metaFile.readText().trim() else "application/octet-stream"
            val mimeType = contentType.split(";").firstOrNull()?.trim() ?: "application/octet-stream"
            val encoding = if (contentType.contains("charset=")) contentType.substringAfter("charset=").trim() else null
            Log.d("EUV_CACHE", "HIT: $url -> ${dataFile.absolutePath} (${dataFile.length()} bytes)")
            return WebResourceResponse(mimeType, encoding, FileInputStream(dataFile))
        } catch (e: Exception) {
            Log.w("EUV_CACHE", "Cache read error: $url", e)
            return null
        }
    }

    private fun writeToCache(url: String, contentType: String, data: ByteArray) {
        try {
            val dataFile = urlToCacheFile(url)
            dataFile.parentFile?.mkdirs()
            dataFile.writeBytes(data)
            getMetaFile(dataFile).writeText(contentType)
            Log.d("EUV_CACHE", "SAVED: $url -> ${dataFile.absolutePath} (${data.size} bytes, type=$contentType)")
        } catch (e: Exception) {
            Log.w("EUV_CACHE", "Cache write error: $url", e)
        }
    }

    /**
     * Fetches a URL from the network, following redirects.
     * Handles gzip-compressed responses by decompressing before caching.
     * Returns the final (post-redirect) URL and the response data.
     */
    private fun fetchFromNetwork(url: String): WebResourceResponse? {
        try {
            var currentUrl = url
            var redirectCount = 0
            val maxRedirects = 10

            while (redirectCount < maxRedirects) {
                val conn = URL(currentUrl).openConnection() as HttpURLConnection
                conn.connectTimeout = 15000
                conn.readTimeout = 30000
                conn.requestMethod = "GET"
                conn.instanceFollowRedirects = false // manual redirect to track final URL
                conn.setRequestProperty("User-Agent", "Mozilla/5.0 (Linux; Android 14) AppleWebKit/537.36")
                conn.setRequestProperty("Accept-Encoding", "gzip, deflate")

                val code = conn.responseCode

                // Handle redirects (301, 302, 303, 307, 308)
                if (code in 300..399) {
                    val location = conn.getHeaderField("Location")
                    conn.disconnect()
                    if (location.isNullOrEmpty()) return null
                    currentUrl = URL(URL(currentUrl), location).toString()
                    redirectCount++
                    Log.d("EUV_CACHE", "REDIRECT: $url -> $currentUrl (code=$code)")
                    continue
                }

                if (code != HttpURLConnection.HTTP_OK) {
                    conn.disconnect()
                    Log.w("EUV_CACHE", "HTTP error: $currentUrl code=$code")
                    return null
                }

                val contentType = conn.contentType ?: "application/octet-stream"
                val contentEncoding = conn.contentEncoding
                val rawStream = conn.inputStream

                // Decompress gzip if needed
                val body = if (contentEncoding != null && contentEncoding.equals("gzip", ignoreCase = true)) {
                    GZIPInputStream(rawStream).readBytes()
                } else {
                    rawStream.readBytes()
                }

                conn.disconnect()

                // Cache using the FINAL URL (after redirects)
                // Strip content-encoding from Content-Type for storage since data is now decompressed
                val cacheContentType = contentType
                writeToCache(currentUrl, cacheContentType, body)

                val mimeType = cacheContentType.split(";").firstOrNull()?.trim() ?: "application/octet-stream"
                val encoding = if (cacheContentType.contains("charset=")) cacheContentType.substringAfter("charset=").trim() else null
                Log.d("EUV_CACHE", "FETCHED: $currentUrl (${body.size} bytes, redirected from=$url)")

                return WebResourceResponse(mimeType, encoding, ByteArrayInputStream(body))
            }

            Log.w("EUV_CACHE", "Too many redirects: $url")
            return null
        } catch (e: Exception) {
            Log.w("EUV_CACHE", "Fetch error: $url", e)
            return null
        }
    }

    /**
     * Check if a URL is one we should cache (matches our remote URL).
     */
    private fun shouldCacheUrl(url: String): Boolean {
        return try {
            val uri = Uri.parse(url)
            // Cache all requests to our remote host (excluding favicon)
            uri.host == REMOTE_HOST && uri.path != "/favicon.ico"
        } catch (e: Exception) {
            false
        }
    }

    override fun shouldInterceptRequest(view: WebView, request: WebResourceRequest): WebResourceResponse? {
        pendingUrlRedirect?.let {
            Handler(Looper.getMainLooper()).post { view.loadUrl(it) }
            pendingUrlRedirect = null
            return null
        }
        val url = request.url.toString()
        lastInterceptedUrl = request.url

        // If the WebView requests the Tauri local asset (index.html),
        // redirect to the remote URL so the WebView loads from the network/cache.
        if (url.startsWith("http://tauri.localhost/")) {
            Log.d("EUV_CACHE", "REDIRECTING tauri.localhost -> $REMOTE_URL")
            Handler(Looper.getMainLooper()).post {
                view.loadUrl(REMOTE_URL)
            }
            // Return empty response to cancel the tauri.localhost load
            return WebResourceResponse("text/html", "UTF-8", ByteArrayInputStream("<html><body>Redirecting...</body></html>".toByteArray()))
        }

        Log.d("EUV_CACHE", "INTERCEPT: $url method=${request.method} shouldCache=${shouldCacheUrl(url)}")

        // Only intercept GET requests to our remote URL
        if (request.method == "GET" && shouldCacheUrl(url)) {
            // Try cache first
            val cached = readFromCache(url)
            if (cached != null) {
                // Cache hit: return cached data immediately, refresh in background
                Thread {
                    Log.d("EUV_CACHE", "Background refresh: $url")
                    fetchFromNetwork(url)
                }.start()
                return cached
            }
            // Cache miss: fetch from network
            val networkResponse = fetchFromNetwork(url)
            if (networkResponse != null) {
                return networkResponse
            }
            // Network failed and no cache: return error page (200 OK so WebView renders it)
            Log.w("EUV_CACHE", "Network failed, no cache for: $url")
            val errorHtml = """<!doctype html>
<html><head><meta charset="UTF-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>Offline</title></head>
<body style="background:#1a1a2e;color:#eee;display:flex;align-items:center;justify-content:center;height:100vh;font-family:sans-serif;margin:0">
<div style="text-align:center"><h1 style="color:#f44">🌐 Offline</h1>
<p>No cached content available.</p><p>Please check your network connection.</p></div></body></html>"""
            val errorBody = errorHtml.toByteArray()
            return WebResourceResponse("text/html", "UTF-8", 200, "OK",
                mapOf("Content-Type" to "text/html; charset=UTF-8"), ByteArrayInputStream(errorBody))
        }

        // For all other requests, use Tauri's normal handling
        if (Rust.withAssetLoader((view as RustWebView).id)) {
            return assetLoader.shouldInterceptRequest(request.url)
        }
        val response = Rust.handleRequest(view.id, request, view.isDocumentStartScriptEnabled)
        if (response != null) {
            response.responseHeaders = (response.responseHeaders ?: emptyMap()) + ("Cache-Control" to "no-store")
        }
        interceptedState[url] = response != null
        return response
    }

    override fun shouldOverrideUrlLoading(view: WebView, request: WebResourceRequest): Boolean {
        return Rust.shouldOverride((view as RustWebView).id, request.url.toString())
    }

    override fun onPageStarted(view: WebView, url: String, favicon: Bitmap?) {
        currentUrl = url
        if (interceptedState[url] == false) {
            val webView = view as RustWebView
            for (script in webView.initScripts) { view.evaluateJavascript(script, null) }
        }
        return Rust.onPageLoading((view as RustWebView).id, url)
    }

    override fun onPageFinished(view: WebView, url: String) {
        Rust.onPageLoaded((view as RustWebView).id, url)
    }

    override fun onReceivedError(view: WebView, request: WebResourceRequest, error: WebResourceError) {
        if (error.errorCode == ERROR_CONNECT && request.isForMainFrame && request.url != lastInterceptedUrl) {
            view.stopLoading()
            view.loadUrl(request.url.toString())
            pendingUrlRedirect = request.url.toString()
        } else {
            super.onReceivedError(view, request, error)
        }
    }
}
'@

$clientFile = "$androidDir\app\src\main\java\com\euv\generated\RustWebViewClient.kt"
$cacheKotlin | Set-Content $clientFile -Encoding UTF8 -Force
Write-Output "Cache logic injected into RustWebViewClient.kt"

# ===== Step 3: Fix keystore path in build.gradle.kts =====
Write-Output ""
Write-Output "=== Step 3: Fix keystore path ==="
$gradleFile = "$androidDir\app\build.gradle.kts"
$gradleContent = Get-Content $gradleFile -Raw
$gradleContent = $gradleContent -replace 'truri-app', 'euv-app'
# Fix usesCleartextTraffic in release build (tauri regenerates as "false")
$gradleContent = $gradleContent -replace '\bmanifestPlaceholders\["usesCleartextTraffic"\] = "false"', 'manifestPlaceholders["usesCleartextTraffic"] = "true"'
$gradleContent | Set-Content $gradleFile -Encoding UTF8 -Force
Write-Output "Fixed keystore path and usesCleartextTraffic in build.gradle.kts"

# ===== Step 4: Repackage APK =====
Write-Output ""
Write-Output "=== Step 4: gradlew assembleRelease ==="
cd $androidDir
.\gradlew assembleRelease --no-daemon

# ===== Step 5: Sign APK =====
Write-Output ""
Write-Output "=== Step 5: Sign APK ==="
$buildTools = "$env:LOCALAPPDATA\Android\Sdk\build-tools"
$apksigner = Get-ChildItem $buildTools -Directory | Sort-Object { [version]($_.Name) } -Descending | Select-Object -First 1 | ForEach-Object { "$($_.FullName)\apksigner.bat" }

$apkSrc = "$androidDir\app\build\outputs\apk\universal\release\app-universal-release.apk"
$apkDst = "$desktop\euv-release.apk"

& $apksigner sign --ks $keystore --ks-key-alias euv --ks-pass pass:euv123456 --key-pass pass:euv123456 --out $apkDst $apkSrc

# ===== Step 6: Verify & Copy =====
Write-Output ""
Write-Output "=== Step 6: Verify ==="
& $apksigner verify --verbose $apkDst

$size = (Get-Item $apkDst).Length / 1MB
Write-Output ""
Write-Output "APK: $([math]::Round($size, 1)) MB"
Write-Output "Path: $apkDst"

