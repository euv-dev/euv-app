use crate::*;

#[derive(Serialize)]
pub(crate) struct CachedPage {
    pub(crate) from_cache: bool,
    pub(crate) remote_url: String,
}
