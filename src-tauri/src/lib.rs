//! Euv-app
//!
//! A Tauri-based application with resource caching capabilities.

mod cache;

use cache::*;

use std::borrow::Cow;
use std::path::PathBuf;

use {
    serde::Serialize,
    tauri::{
        App, AppHandle, Builder, Manager, RunEvent, WebviewUrl, WebviewWindowBuilder,
        async_runtime::spawn, generate_context, generate_handler,
    },
};

/// Represents a cached web page result with fallback URL.
#[derive(Serialize)]
pub(crate) struct CachedPage {
    /// Whether the page was loaded from local cache.
    pub(crate) from_cache: bool,
    /// The remote URL to use as fallback when cache is not available.
    pub(crate) remote_url: String,
}

/// Initializes and runs the Tauri application.
pub fn run() {
    #[cfg(target_os = "android")]
    {
        tauri::android_binding!(com, euv, run, tauri::wry);
    }
    Builder::default()
        .setup(|app: &mut App| {
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }
            let handle: AppHandle = app.handle().clone();
            let has_cache: bool = cache::load_cached_html(&handle).is_some();
            if has_cache {
                log::info!("[EUV] cache found, loading from cache directly");
                let scheme_url: String = get_scheme_url("index.html");
                let _window: tauri::WebviewWindow = WebviewWindowBuilder::new(
                    app,
                    "main",
                    WebviewUrl::External(scheme_url.parse().unwrap()),
                )
                .title("Euv")
                .inner_size(1280.0, 800.0)
                .resizable(true)
                .build()?;
                spawn(async move {
                    cache::update_cache_async(handle).await;
                });
            } else {
                log::info!("[EUV] no cache, starting with loading page");
                let _window: tauri::WebviewWindow =
                    WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
                        .title("Euv")
                        .inner_size(1280.0, 800.0)
                        .resizable(true)
                        .build()?;
                let handle_for_fetch: AppHandle = app.handle().clone();
                spawn(async move {
                    cache::initial_fetch_and_notify(handle_for_fetch).await;
                });
            }
            Ok(())
        })
        .register_uri_scheme_protocol(SCHEME_NAME, move |ctx, request| {
            handle_euv_scheme(ctx.app_handle(), request)
        })
        .invoke_handler(generate_handler![load_cached_resource])
        .build(generate_context!())
        .expect("fatal: euv app failed to start")
        .run(|_app_handle: &AppHandle, event: RunEvent| {
            if let RunEvent::Exit = event {
                log::info!("[EUV] app exiting");
            }
        });
}

/// Builds the custom scheme URL for a given path.
pub(crate) fn get_scheme_url(path: &str) -> String {
    #[cfg(target_os = "android")]
    {
        format!("https://euv.localhost/{}", path)
    }
    #[cfg(not(target_os = "android"))]
    {
        if cfg!(target_os = "windows") {
            format!("https://euv.localhost/{}", path)
        } else {
            format!("euv://localhost/{}", path)
        }
    }
}
