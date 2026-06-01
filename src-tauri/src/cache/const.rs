/// The remote URL to fetch the resource from.
pub const REMOTE_URL: &str = "https://ltpp.vip/euv";

/// Subdirectory under app_cache_dir for cached web resources.
pub const CACHE_DIR: &str = "euv_web_cache";

/// Timeout in seconds for remote fetch requests.
pub const FETCH_TIMEOUT_SECS: u64 = 30;

/// Maximum response body size in bytes (10 MB).
pub const MAX_BODY_SIZE: usize = 10 * 1024 * 1024;
