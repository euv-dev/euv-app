include!(concat!(env!("OUT_DIR"), "/bundled_cache_data.rs"));

mod cache;

use cache::*;

use serde::Serialize;

use {
    reqwest::{Client, redirect::Policy},
    std::{
        borrow::Cow,
        path::{Path, PathBuf},
        sync::{Mutex, OnceLock},
        time::{Duration, Instant, SystemTime, UNIX_EPOCH},
    },
    tauri::{
        App, AppHandle, Builder, RunEvent, async_runtime::spawn, generate_context, generate_handler,
    },
    tokio::fs::{
        create_dir_all, metadata, read_dir, read_to_string, remove_dir_all, rename, write,
    },
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
            ensure_serving_version_sync(&handle);
            spawn(async move {
                if load_cached_html(&handle).await.is_none() {
                    log::info!("[EUV] no cache, deploying bundled");
                    deploy_bundled_cache(&handle).await;
                }
                if load_cached_html(&handle).await.is_some() {
                    log::info!("[EUV] cache ready, background update");
                    update_cache_async(handle).await;
                } else {
                    log::info!("[EUV] no cache, initial fetch");
                    initial_fetch_and_notify(handle).await;
                }
            });
            Ok(())
        })
        .register_uri_scheme_protocol(SCHEME_NAME, move |ctx, req| {
            handle_euv_scheme(ctx.app_handle(), req)
        })
        .invoke_handler(generate_handler![load_cached_resource])
        .build(generate_context!())
        .expect("fatal: app failed to start")
        .run(|_: &AppHandle, _: RunEvent| {});
}
