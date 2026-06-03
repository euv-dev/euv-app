mod cache;

use cache::*;
use tauri::{App, AppHandle, Builder, RunEvent, async_runtime::spawn, generate_context, generate_handler};

/// Synchronously deploy bundled cache and set it as the serving version.
/// The scheme handler always serves the bundled version to guarantee completeness.
/// Background updates only pre-warm the cache for future use but never affect the
/// current process's serving version.
fn ensure_serving_version_sync(app_handle: &AppHandle) {
    // If serving version is already set, nothing to do
    if get_serving_version().is_some() { return; }

    let bundled_files = crate::cache::bundled::BUNDLED_FILES;
    if bundled_files.is_empty() { return; }

    let cache_root = match get_cache_root(app_handle) {
        Ok(d) => d,
        Err(_) => return,
    };
    std::fs::create_dir_all(&cache_root).ok();

    // Always deploy bundled files to a fresh version directory
    let version_name = format!("{}{}",
        VERSION_PREFIX,
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis()
    );
    let version_dir = cache_root.join(&version_name);
    if std::fs::create_dir_all(&version_dir).is_err() { return; }

    let mut count: usize = 0;
    for (rel_path, data) in bundled_files {
        let target = version_dir.join(rel_path);
        if let Some(parent) = target.parent() { std::fs::create_dir_all(parent).ok(); }
        if std::fs::write(&target, data).is_ok() { count += 1; }
    }
    if count == 0 { return; }

    set_serving_version(&version_name);
    log::info!("[EUV] deployed {} bundled files sync, serving: {}", count, version_name);
}

pub fn run() {
    #[cfg(target_os = "android")]
    { tauri::android_binding!(com, euv, run, tauri::wry); }
    Builder::default()
        .setup(|app: &mut App| {
            app.handle().plugin(tauri_plugin_log::Builder::default().level(log::LevelFilter::Info).build())?;
            let handle: AppHandle = app.handle().clone();

            // Synchronously ensure a serving version before WebView starts loading
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
