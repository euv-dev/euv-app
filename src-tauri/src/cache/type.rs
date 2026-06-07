/// The result type for fetching a list of resources: a vector of (clean_path, data) tuples.
pub(crate) type FetchResult = Vec<(String, Vec<u8>)>;
