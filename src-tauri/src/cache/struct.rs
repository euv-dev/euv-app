use crate::*;

#[derive(Serialize)]
pub(crate) struct CachedPage {
    pub(crate) from_cache: bool,
    pub(crate) remote_url: String,
    pub(crate) source: String,
    pub(crate) cache_path: String,
}
