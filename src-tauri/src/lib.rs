//! Euv-app
//!
//! A Tauri-based application with resource caching capabilities.
//! Supports offline-first launch via bundled resources embedded at build time.

mod cache;

use cache::*;

use std::borrow::Cow;
use std::path::PathBuf;

use {
    serde::Serialize,
    tauri::{
        App, AppHandle, Builder, Manager, RunEvent, async_runtime::spawn, generate_context,
        generate_handler,
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
            app.handle().plugin(
                tauri_plugin_log::Builder::default()
                    .level(log::LevelFilter::Info)
                    .build(),
            )?;
            let handle: AppHandle = app.handle().clone();

            // Check if runtime cache exists
            let has_cache: bool = cache::load_cached_html(&handle).is_some();

            if !has_cache {
                // No runtime cache — try to deploy bundled (built-in) resources
                log::info!("[EUV] no runtime cache, deploying bundled resources");
                let deployed: bool = cache::deploy_bundled_cache(&handle);
                if deployed {
                    log::info!("[EUV] bundled cache deployed successfully");
                } else {
                    log::info!("[EUV] no bundled cache available, will fetch from network");
                }
            }

            // Re-check after potential bundled deploy
            let has_cache_now: bool = cache::load_cached_html(&handle).is_some();

            if has_cache_now {
                // Cache available — the loading page (dist/index.html) will poll
                // load_cached_resource, find cache ready, and navigate to euv://
                log::info!("[EUV] cache available, frontend will navigate when ready");

                // Background: async update cache from network
                spawn(async move {
                    cache::update_cache_async(handle).await;
                });
            } else {
                // No cache at all — loading page will poll until cache is fetched
                log::info!("[EUV] no cache, starting network fetch");
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
        .run(|_: &AppHandle, event: RunEvent| {
            if let RunEvent::Exit = event {
                log::info!("[EUV] app exiting");
            }
        });
}
