mod cache;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    println!("[EUV] Rust run() started");
    let result = tauri::Builder::default()
        .setup(|app| {
            println!("[EUV] setup() called");
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }
            // Kick off background cache update
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                cache::update_cache_async(handle).await;
            });
            println!("[EUV] setup() done");
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![cache::load_cached_resource])
        .run(tauri::generate_context!());

    match result {
        Ok(_) => println!("[EUV] tauri run() completed"),
        Err(e) => println!("[EUV] tauri run() error: {:?}", e),
    }
}
