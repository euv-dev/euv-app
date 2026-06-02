use serde::Serialize;

#[derive(Debug)]
pub(crate) enum CacheError {
    Fetch(String),
    Write(String),
    Read(String),
}

#[derive(Serialize)]
pub(crate) struct CachedPage {
    pub(crate) from_cache: bool,
    pub(crate) remote_url: String,
}
