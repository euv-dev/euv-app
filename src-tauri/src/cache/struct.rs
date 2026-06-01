use std::collections::HashMap;

/// Represents a cached web page with all its resources.
#[derive(serde::Serialize)]
pub struct CachedPage {
    /// The HTML content with rewritten resource URLs pointing to local files.
    pub html: String,
    /// Whether this was loaded from cache (true) or freshly fetched (false).
    pub from_cache: bool,
    /// Number of resources cached.
    pub resource_count: usize,
}

/// Error types for cache operations.
#[derive(Debug)]
pub enum CacheError {
    Fetch(String),
    Write(String),
    Read(String),
    Parse(String),
}

impl std::fmt::Display for CacheError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CacheError::Fetch(msg) => write!(f, "Fetch error: {}", msg),
            CacheError::Write(msg) => write!(f, "Write error: {}", msg),
            CacheError::Read(msg) => write!(f, "Read error: {}", msg),
            CacheError::Parse(msg) => write!(f, "Parse error: {}", msg),
        }
    }
}
