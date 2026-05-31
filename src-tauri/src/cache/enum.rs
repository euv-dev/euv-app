/// Represents errors that can occur during cache operations.
#[derive(Debug)]
pub enum CacheError {
    /// Failed to read from the local cache file.
    Read(String),
    /// Failed to write to the local cache file.
    Write(String),
    /// Failed to fetch the remote resource.
    Fetch(String),
}
