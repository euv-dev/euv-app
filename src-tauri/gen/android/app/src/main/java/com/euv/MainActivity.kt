package com.euv

import android.os.Bundle
import android.util.Log
import android.webkit.WebView
import android.webkit.WebChromeClient
import android.webkit.ConsoleMessage

class MainActivity : TauriActivity() {

    override fun onCreate(savedInstanceState: Bundle?) {
        Log.d("EUV_CACHE", "MainActivity.onCreate()")
        super.onCreate(savedInstanceState)
    }

    override fun onWebViewCreate(webView: WebView) {
        Log.d("EUV_CACHE", "onWebViewCreate()")
        super.onWebViewCreate(webView)

        webView.settings.apply {
            @Suppress("DEPRECATION")
            allowFileAccessFromFileURLs = true
            @Suppress("DEPRECATION")
            allowUniversalAccessFromFileURLs = true
            allowFileAccess = true
        }

        webView.webChromeClient = object : WebChromeClient() {
            override fun onConsoleMessage(msg: ConsoleMessage): Boolean {
                Log.d("JSConsole", "${msg.message()} [line ${msg.lineNumber()}]")
                return true
            }
        }

        Log.d("EUV_CACHE", "WebView setup done")
    }
}
