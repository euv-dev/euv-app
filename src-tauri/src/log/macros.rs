#[macro_export]
macro_rules! euv_log {
    ($($arg:tt)*) => {{
        let msg = format!($($arg)*);
        ::log::info!("{}", msg);
        #[cfg(debug_assertions)]
        {
            if let Some(handle) = $crate::APP_HANDLE.get() {
                use tauri::Emitter;
                let _ = handle.emit("euv://debug-log", msg);
            }
        }
    }};
}
