package com.euv

import android.Manifest
import android.content.Intent
import android.content.pm.PackageManager
import android.graphics.Color
import android.graphics.drawable.ColorDrawable
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.util.Log
import android.view.Gravity
import android.view.View
import android.view.ViewGroup
import android.graphics.Paint
import android.view.SurfaceControl
import android.view.WindowManager
import android.webkit.JavascriptInterface
import android.webkit.WebView
import android.webkit.WebChromeClient
import android.webkit.ConsoleMessage
import android.widget.FrameLayout
import android.widget.ImageView
import androidx.activity.result.contract.ActivityResultContracts
import androidx.core.content.ContextCompat
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import androidx.core.view.WindowInsetsControllerCompat

class MainActivity : TauriActivity() {

    private var splashView: View? = null
    private val notificationPermissionLauncher = registerForActivityResult(
        ActivityResultContracts.RequestPermission()
    ) { granted: Boolean ->
        if (granted) {
            startKeepAliveService()
        } else {
            Log.w("EUV_CACHE", "POST_NOTIFICATIONS permission denied, keep-alive service not started")
        }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        Log.d("EUV_CACHE", "MainActivity.onCreate()")
        try {
            val field = RustWebViewClient::class.java.getDeclaredField("mainFrameLoaded")
            field.isAccessible = true
            field.set(null, false)
            Log.d("EUV_CACHE", "Reset mainFrameLoaded via reflection")
        } catch (e: Exception) {
            Log.d("EUV_CACHE", "mainFrameLoaded field not found (expected on vanilla Tauri)")
        }
        window.setBackgroundDrawable(ColorDrawable(Color.parseColor(AppConfig.BACKGROUND_COLOR)))
        super.onCreate(savedInstanceState)

        // Add splash overlay IMMEDIATELY after super.onCreate() so it covers the
        // white gap between Android system splash dismissal and WebView rendering.
        // This must happen here (not in onWebViewCreate) to avoid the flash.
        addSplashOverlay()

        if (AppConfig.IMMERSIVE_MODE) {
            enableImmersiveMode()
        }
        setMaxFrameRate()
        if (AppConfig.KEEP_ALIVE_SERVICE) {
            startKeepAliveServiceSafely()
        }
    }

    private fun startKeepAliveServiceSafely() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            when {
                ContextCompat.checkSelfPermission(this, Manifest.permission.POST_NOTIFICATIONS) == PackageManager.PERMISSION_GRANTED -> {
                    startKeepAliveService()
                }
                shouldShowRequestPermissionRationale(Manifest.permission.POST_NOTIFICATIONS) -> {
                    Log.w("EUV_CACHE", "User previously denied POST_NOTIFICATIONS, requesting again")
                    notificationPermissionLauncher.launch(Manifest.permission.POST_NOTIFICATIONS)
                }
                else -> {
                    notificationPermissionLauncher.launch(Manifest.permission.POST_NOTIFICATIONS)
                }
            }
        } else {
            startKeepAliveService()
        }
    }

    private fun startKeepAliveService() {
        val serviceIntent = Intent(this, KeepAliveService::class.java)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            startForegroundService(serviceIntent)
        } else {
            startService(serviceIntent)
        }
        Log.d("EUV_CACHE", "KeepAliveService started")
    }

    private fun addSplashOverlay() {
        val rootView = window.decorView as ViewGroup
        val splash = FrameLayout(this).apply {
            setBackgroundColor(Color.parseColor(AppConfig.BACKGROUND_COLOR))
            layoutParams = FrameLayout.LayoutParams(
                FrameLayout.LayoutParams.MATCH_PARENT,
                FrameLayout.LayoutParams.MATCH_PARENT
            )
            elevation = 100f

            // Logo image centered
            val logo = ImageView(context).apply {
                setImageResource(R.mipmap.ic_launcher)
                layoutParams = FrameLayout.LayoutParams(
                    240, 240, Gravity.CENTER
                )
                scaleType = ImageView.ScaleType.FIT_CENTER
            }
            addView(logo)
        }
        rootView.addView(splash)
        splashView = splash
        Log.d("EUV_CACHE", "Splash overlay added to DecorView")
    }

    fun removeSplash() {
        splashView?.let { splash ->
            splash.animate()
                .alpha(0f)
                .setDuration(AppConfig.SPLASH_FADE_DURATION_MS)
                .withEndAction {
                    (splash.parent as? ViewGroup)?.removeView(splash)
                    splashView = null
                    Log.d("EUV_CACHE", "Splash removed")
                }
                .start()
        }
    }

    private fun enableImmersiveMode() {
        WindowCompat.setDecorFitsSystemWindows(window, false)
        val controller = WindowInsetsControllerCompat(window, window.decorView)
        controller.hide(WindowInsetsCompat.Type.systemBars())
        controller.systemBarsBehavior = WindowInsetsControllerCompat.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.R) {
            @Suppress("DEPRECATION")
            window.decorView.systemUiVisibility = (
                View.SYSTEM_UI_FLAG_IMMERSIVE_STICKY
                or View.SYSTEM_UI_FLAG_FULLSCREEN
                or View.SYSTEM_UI_FLAG_HIDE_NAVIGATION
                or View.SYSTEM_UI_FLAG_LAYOUT_STABLE
                or View.SYSTEM_UI_FLAG_LAYOUT_FULLSCREEN
                or View.SYSTEM_UI_FLAG_LAYOUT_HIDE_NAVIGATION
            )
        }
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
            window.attributes.layoutInDisplayCutoutMode =
                WindowManager.LayoutParams.LAYOUT_IN_DISPLAY_CUTOUT_MODE_SHORT_EDGES
        }
    }

    override fun onWindowFocusChanged(hasFocus: Boolean) {
        super.onWindowFocusChanged(hasFocus)
        if (hasFocus && AppConfig.IMMERSIVE_MODE) {
            enableImmersiveMode()
        }
        if (hasFocus) {
            setMaxFrameRate()
        }
    }

    private fun setMaxFrameRate() {
        if (!AppConfig.MAX_FRAME_RATE_ENABLED) return
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
            val display = display ?: return
            val supportedModes = display.supportedModes
            val maxRefreshMode = supportedModes.maxByOrNull { it.refreshRate }
            if (maxRefreshMode != null) {
                val params: WindowManager.LayoutParams = window.attributes
                params.preferredDisplayModeId = maxRefreshMode.modeId
                window.attributes = params
                Log.d("EUV_CACHE", "Set preferred display mode to ${maxRefreshMode.refreshRate}Hz (modeId=${maxRefreshMode.modeId})")
            }
            try {
                val surfaceControl = window.decorView.rootSurfaceControl
                if (surfaceControl != null && maxRefreshMode != null) {
                    val setFrameRateMethod = surfaceControl.javaClass.getMethod(
                        "setFrameRate",
                        Float::class.javaPrimitiveType,
                        Int::class.javaPrimitiveType,
                        Int::class.javaPrimitiveType
                    )
                    val frameRateCompatibilityFixedSource = SurfaceControl::class.java
                        .getDeclaredField("FRAME_RATE_COMPATIBILITY_FIXED_SOURCE")
                        .getInt(null)
                    val changeFrameRateOnlyIfSeamless = SurfaceControl::class.java
                        .getDeclaredField("CHANGE_FRAME_RATE_ONLY_IF_SEAMLESS")
                        .getInt(null)
                    setFrameRateMethod.invoke(
                        surfaceControl,
                        maxRefreshMode.refreshRate,
                        frameRateCompatibilityFixedSource,
                        changeFrameRateOnlyIfSeamless
                    )
                    Log.d("EUV_CACHE", "Set frame rate to ${maxRefreshMode.refreshRate}fps via SurfaceControl")
                }
            } catch (e: Exception) {
                Log.w("EUV_CACHE", "SurfaceControl.setFrameRate failed: ${e.message}")
            }
        } else if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.M) {
            @Suppress("DEPRECATION")
            val display = windowManager.defaultDisplay
            val supportedModes = display.supportedModes
            val maxRefreshMode = supportedModes.maxByOrNull { it.refreshRate }
            if (maxRefreshMode != null) {
                val params: WindowManager.LayoutParams = window.attributes
                params.preferredDisplayModeId = maxRefreshMode.modeId
                window.attributes = params
                Log.d("EUV_CACHE", "Set preferred display mode to ${maxRefreshMode.refreshRate}Hz (modeId=${maxRefreshMode.modeId}, API <31)")
            }
        }
    }

    /**
     * JavaScript interface to allow the WebView content to open external links
     * in the system browser.
     */
    inner class ExternalLinkHandler {
        @JavascriptInterface
        fun openUrl(url: String) {
            Log.d("EUV_CACHE", "Opening external link: $url")
            try {
                val intent = Intent(Intent.ACTION_VIEW, Uri.parse(url))
                startActivity(intent)
            } catch (e: Exception) {
                Log.e("EUV_CACHE", "Failed to open external link: $url", e)
            }
        }
    }

    override fun onWebViewCreate(webView: WebView) {
        Log.d("EUV_CACHE", "onWebViewCreate() at ${System.currentTimeMillis()}")
        super.onWebViewCreate(webView)
        webView.settings.apply {
            @Suppress("DEPRECATION")
            allowFileAccessFromFileURLs = true
            @Suppress("DEPRECATION")
            allowUniversalAccessFromFileURLs = true
            allowFileAccess = true
            domStorageEnabled = true
            javaScriptEnabled = true
            @Suppress("DEPRECATION")
            setRenderPriority(android.webkit.WebSettings.RenderPriority.HIGH)
            @Suppress("DEPRECATION")
            layoutAlgorithm = android.webkit.WebSettings.LayoutAlgorithm.NORMAL
            blockNetworkImage = false
            loadsImagesAutomatically = true
            cacheMode = android.webkit.WebSettings.LOAD_DEFAULT
        }
        if (AppConfig.ANTI_ALIASING) {
            val paint: Paint = Paint().apply {
                isAntiAlias = true
                isFilterBitmap = true
                isDither = true
                isSubpixelText = true
            }
            webView.setLayerType(View.LAYER_TYPE_HARDWARE, paint)
        } else {
            webView.setLayerType(View.LAYER_TYPE_HARDWARE, null)
        }
        webView.setBackgroundColor(Color.parseColor(AppConfig.BACKGROUND_COLOR))
        webView.webChromeClient = object : WebChromeClient() {
            override fun onConsoleMessage(msg: ConsoleMessage): Boolean {
                Log.d("JSConsole", "${msg.message()} [line ${msg.lineNumber()}]")
                return true
            }
        }

        // Inject CSS anti-aliasing and text rendering optimizations
        if (AppConfig.ANTI_ALIASING) {
            webView.evaluateJavascript("""
                (function() {
                    var style = document.createElement('style');
                    style.textContent = '
                        * { -webkit-font-smoothing: antialiased; -moz-osx-font-smoothing: grayscale; text-rendering: optimizeLegibility; }
                        canvas { image-rendering: auto; }
                    ';
                    document.head.appendChild(style);
                })();
            """.trimIndent(), null)
        }

        // Add JavaScript interface for opening external links
        webView.addJavascriptInterface(ExternalLinkHandler(), "NativeApp")

        // Poll WebView: wait for euv.localhost page to fully render before removing splash.
        // We check that the URL has navigated to euv.localhost AND that the page body has
        // meaningful content (WASM app has rendered), to avoid a white flash.
        val handler = Handler(Looper.getMainLooper())
        val pollRunnable = object : Runnable {
            override fun run() {
                val currentUrl = webView.url ?: ""
                if (currentUrl.contains("euv.localhost")) {
                    // Check if WASM app has rendered content into the page
                    webView.evaluateJavascript(
                        "(document.querySelector('canvas') !== null || document.body.children.length > 1).toString()"
                    ) { result ->
                        val rendered = result?.trim('"') == "true"
                        Log.d("EUV_CACHE", "Splash poll: url=$currentUrl, rendered=$rendered")
                        if (rendered) {
                            Log.d("EUV_CACHE", "WASM app rendered, removing splash")
                            removeSplash()
                            injectExternalLinkInterceptor(webView)
                        } else {
                            handler.postDelayed(this, 100)
                        }
                    }
                } else {
                    handler.postDelayed(this, 200)
                }
            }
        }
        handler.postDelayed(pollRunnable, 300)

        Log.d("EUV_CACHE", "WebView setup done at ${System.currentTimeMillis()}")
    }

    private fun injectExternalLinkInterceptor(webView: WebView) {
        val js = """
            (function() {
                if (window.__externalLinkInterceptorInstalled) return;
                window.__externalLinkInterceptorInstalled = true;
                document.addEventListener('click', function(e) {
                    var target = e.target;
                    while (target && target.tagName !== 'A') {
                        target = target.parentElement;
                    }
                    if (target && target.href) {
                        var href = target.href;
                        if (href.indexOf('euv.localhost') === -1 &&
                            href.indexOf('euv://') !== 0 &&
                            href.indexOf('tauri.localhost') === -1 &&
                            (href.indexOf('http://') === 0 || href.indexOf('https://') === 0)) {
                            e.preventDefault();
                            e.stopPropagation();
                            window.NativeApp.openUrl(href);
                        }
                    }
                }, true);
                // Also handle window.open
                var originalOpen = window.open;
                window.open = function(url, target, features) {
                    if (url && url.indexOf('http') === 0 &&
                        url.indexOf('euv.localhost') === -1 &&
                        url.indexOf('tauri.localhost') === -1) {
                        window.NativeApp.openUrl(url);
                        return null;
                    }
                    return originalOpen.call(window, url, target, features);
                };
                console.log('[EUV] External link interceptor installed');
            })();
        """.trimIndent()
        webView.evaluateJavascript(js, null)
        Log.d("EUV_CACHE", "External link interceptor JS injected")
    }
}
