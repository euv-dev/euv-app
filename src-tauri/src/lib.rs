//! App Lib
//!
//! Tauri application with resource caching for fast startup.

mod cache;

pub use {cache::load_cached_resource, cache::update_cache_async};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }
            let app_handle: tauri::AppHandle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                update_cache_async(app_handle).await;
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![load_cached_resource])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
