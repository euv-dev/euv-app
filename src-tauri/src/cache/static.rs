use super::*;

/// A global handle to the Tauri `AppHandle`, available only in debug builds.
///
/// Used by the `euv_log!` macro to emit debug log events to the frontend debug panel.
#[cfg(debug_assertions)]
pub(crate) static APP_HANDLE: OnceLock<AppHandle> = OnceLock::new();

/// A lazily-initialized global HTTP client that reuses connection pools across all fetch operations.
///
/// Avoids the overhead of creating a new `Client` instance per request, which would
/// re-establish TLS sessions and DNS resolutions each time.
pub(crate) static HTTP_CLIENT: OnceLock<Client> = OnceLock::new();

/// A watch channel that signals when the background update (or initial fetch) has completed.
///
/// The scheme handler waits for this value to become `true` before serving the first
/// `index.html` request, ensuring the WebView loads the latest remote resources on cold start.
/// Uses `watch` instead of `Notify` to avoid missed-signal races.
pub(crate) static UPDATE_READY: OnceLock<(
    tokio::sync::watch::Sender<bool>,
    tokio::sync::watch::Receiver<bool>,
)> = OnceLock::new();
