/// Error types for cache operations.
#[derive(Debug)]
pub(crate) enum CacheError {
    /// Error occurred during network fetch.
    Fetch(String),
    /// Error occurred during file write.
    Write(String),
    /// Error occurred during file read.
    Read(String),
}
