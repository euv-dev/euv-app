use super::*;

/// Represents the result of loading a cached or remote resource.
#[derive(Serialize)]
pub struct LoadResult {
    /// The HTML content to display.
    pub content: String,
    /// Whether the content was loaded from the local cache.
    pub from_cache: bool,
}
