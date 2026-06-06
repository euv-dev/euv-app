include!(concat!(env!("OUT_DIR"), "/bundled_cache_data.rs"));

#[macro_use]
mod macros;

mod cache;

use cache::*;

use std::{
    borrow::Cow,
    path::{Path, PathBuf},
    sync::{Arc, OnceLock},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use {
    reqwest::{Client, redirect::Policy},
    serde::Serialize,
    tauri::{
        App, AppHandle, Builder, Manager, RunEvent, async_runtime::spawn, generate_context,
        generate_handler,
    },
    tokio::fs::{create_dir_all, read_dir, read_to_string, remove_dir_all, rename, write},
};

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
            #[cfg(debug_assertions)]
            let _ = APP_HANDLE.set(handle.clone());
            deploy_bundled_cache_sync(&handle);
            spawn(async move {
                if has_active_cache(&handle) {
                    euv_log!("[EUV] cache ready, background update");
                    background_update(handle).await;
                } else {
                    euv_log!("[EUV] no cache, initial fetch");
                    initial_fetch(handle).await;
                }
            });
            Ok(())
        })
        .register_uri_scheme_protocol(
            SCHEME_NAME,
            move |context: tauri::UriSchemeContext<'_, tauri::Wry>,
                  request: http::Request<Vec<u8>>| {
                handle_euv_scheme(context.app_handle(), request)
            },
        )
        .invoke_handler(generate_handler![load_cached_resource])
        .build(generate_context!())
        .expect("fatal: app failed to start")
        .run(|_: &AppHandle, _: RunEvent| {});
}
