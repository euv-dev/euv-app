package com.euv

import android.os.Bundle
import android.util.Log
import android.webkit.WebView
import android.webkit.WebChromeClient
import android.webkit.ConsoleMessage
import android.webkit.WebResourceRequest
import android.webkit.WebResourceResponse
import java.io.ByteArrayInputStream

class MainActivity : TauriActivity() {
  override fun onCreate(savedInstanceState: Bundle?) {
    Log.e("EUV_KOTLIN", "MainActivity.onCreate() called")
    super.onCreate(savedInstanceState)
    Log.e("EUV_KOTLIN", "MainActivity.onCreate() done")
  }

  override fun onWebViewCreate(webView: WebView) {
    Log.e("EUV_KOTLIN", "onWebViewCreate() called")
    super.onWebViewCreate(webView)
    Log.e("EUV_KOTLIN", "super.onWebViewCreate() done")
    
    webView.settings.allowFileAccessFromFileURLs = true
    webView.settings.allowUniversalAccessFromFileURLs = true
    webView.settings.allowFileAccess = true
    
    // Override the WebViewClient to intercept and log requests
    webView.webViewClient = object : android.webkit.WebViewClient() {
      override fun shouldInterceptRequest(view: WebView, request: WebResourceRequest): WebResourceResponse? {
        Log.e("EUV_INTERCEPT", "Request: " + request.url.toString())
        return super.shouldInterceptRequest(view, request)
      }
      
      override fun onPageStarted(view: WebView, url: String, favicon: android.graphics.Bitmap?) {
        Log.e("EUV_PAGE", "Page started: " + url)
        super.onPageStarted(view, url, favicon)
      }
      
      override fun onPageFinished(view: WebView, url: String) {
        Log.e("EUV_PAGE", "Page finished: " + url)
        super.onPageFinished(view, url)
      }
      
      override fun onReceivedError(view: WebView, request: WebResourceRequest, error: android.webkit.WebResourceError) {
        Log.e("EUV_ERROR", "Error: " + error.errorCode + " " + error.description + " for " + request.url)
        super.onReceivedError(view, request, error)
      }
    }
    
    webView.webChromeClient = object : WebChromeClient() {
      override fun onConsoleMessage(msg: ConsoleMessage): Boolean {
        Log.e("JSConsole", msg.message() + " [line " + msg.lineNumber() + "]")
        return true
      }
    }
    
    Log.e("EUV_KOTLIN", "WebView setup done")
  }
}
