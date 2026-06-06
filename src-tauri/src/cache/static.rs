use super::*;

/// A global handle to the Tauri `AppHandle`, available only in debug builds.
///
/// Used by the `euv_log!` macro to emit debug log events to the frontend debug panel.
#[cfg(debug_assertions)]
pub(crate) static APP_HANDLE: OnceLock<AppHandle> = OnceLock::new();
