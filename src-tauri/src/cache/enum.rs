/// Represents errors that can occur during cache operations.
///
/// Each variant carries a human-readable description string.
#[derive(Debug)]
pub(crate) enum CacheError {
    /// An error occurred while fetching a remote resource.
    Fetch(String),
    /// An error occurred while writing to the local filesystem.
    Write(String),
    /// An error occurred while reading from the local filesystem.
    Read(String),
}
