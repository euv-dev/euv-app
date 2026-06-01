use crate::*;

/// Represents a cached web page with all its resources.
#[derive(Serialize)]
pub(crate) struct CachedPage {
    /// The HTML content with rewritten resource URLs pointing to local files.
    pub(crate) html: String,
    /// Whether this was loaded from cache (true) or freshly fetched (false).
    pub(crate) from_cache: bool,
    /// Number of resources cached.
    pub(crate) resource_count: usize,
}

/// Error types for cache operations.
#[derive(Debug)]
pub(crate) enum CacheError {
    Fetch(String),
    Write(String),
    Read(String),
}
