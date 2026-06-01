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
            println!("[EUV] setup() done");
            Ok(())
        })
        .run(tauri::generate_context!());

    match result {
        Ok(_) => println!("[EUV] tauri run() completed"),
        Err(e) => println!("[EUV] tauri run() error: {:?}", e),
    }
}
