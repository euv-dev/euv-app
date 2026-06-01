package com.euv

import android.Manifest
import android.content.Intent
import android.content.pm.PackageManager
import android.graphics.Color
import android.graphics.drawable.ColorDrawable
import android.os.Build
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.util.Log
import android.view.Gravity
import android.view.View
import android.view.ViewGroup
import android.view.WindowManager
import android.webkit.WebView
import android.webkit.WebChromeClient
import android.webkit.ConsoleMessage
import android.widget.FrameLayout
import android.widget.ProgressBar
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
        if (AppConfig.IMMERSIVE_MODE) {
            enableImmersiveMode()
        }
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
            val spinner = ProgressBar(context).apply {
                layoutParams = FrameLayout.LayoutParams(
                    96, 96, Gravity.CENTER
                )
                isIndeterminate = true
            }
            addView(spinner)
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
        webView.setLayerType(View.LAYER_TYPE_HARDWARE, null)
        webView.setBackgroundColor(Color.parseColor(AppConfig.BACKGROUND_COLOR))
        webView.webChromeClient = object : WebChromeClient() {
            override fun onConsoleMessage(msg: ConsoleMessage): Boolean {
                Log.d("JSConsole", "${msg.message()} [line ${msg.lineNumber()}]")
                return true
            }
        }
        Handler(Looper.getMainLooper()).post {
            addSplashOverlay()
        }
        Log.d("EUV_CACHE", "WebView setup done at ${System.currentTimeMillis()}")
    }
}
