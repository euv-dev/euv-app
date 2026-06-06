use std::sync::Arc;

pub(crate) type FetchResult = Vec<(String, Vec<u8>)>;
pub(crate) type SharedResults = Arc<tokio::sync::Mutex<FetchResult>>;
