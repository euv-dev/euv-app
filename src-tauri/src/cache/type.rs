use super::*;

/// The result type for fetching a list of resources: a vector of (clean_path, data) tuples.
pub(crate) type FetchResult = Vec<(String, Vec<u8>)>;

/// A shared, thread-safe container for collecting fetch results across concurrent tasks.
pub(crate) type SharedResults = Arc<tokio::sync::Mutex<FetchResult>>;
