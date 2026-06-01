//! Euv-app
//!
//! A Tauri-based application with resource caching capabilities.

mod cache;

use cache::*;

use std::path::PathBuf;

use {
    serde::Serialize,
    tauri::{
        App, AppHandle, Builder, Manager, async_runtime::spawn, generate_context, generate_handler,
    },
};

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
            spawn(async move {
                cache::update_cache_async(handle).await;
            });
            Ok(())
        })
        .invoke_handler(generate_handler![cache::load_cached_resource])
        .run(generate_context!())
        .expect("fatal: euv app failed to start");
}
