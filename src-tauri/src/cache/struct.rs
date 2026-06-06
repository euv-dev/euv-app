use super::*;

/// Represents cached page metadata returned by the `load_cached_resource` Tauri command.
#[derive(Serialize)]
pub(crate) struct CachedPage {
    /// Whether the page was served from the local cache.
    pub(crate) from_cache: bool,
    /// The remote URL of the cached page.
    pub(crate) remote_url: String,
    /// The cache source status (`"active"` or `"none"`).
    pub(crate) source: String,
    /// The local filesystem path to the active cache directory.
    pub(crate) cache_path: String,
}
