/// The remote URL to fetch the resource from.
pub(crate) const REMOTE_URL: &str = "https://ltpp.vip/euv";

/// Subdirectory under app_cache_dir for cached web resources.
pub(crate) const CACHE_DIR: &str = "euv_web_cache";

/// Timeout in seconds for remote fetch requests.
pub(crate) const FETCH_TIMEOUT_SECS: u64 = 30;

/// Maximum response body size in bytes.
pub(crate) const MAX_BODY_SIZE: usize = 10485760;
