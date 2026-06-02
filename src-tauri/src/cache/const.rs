/// The remote URL to fetch the main page HTML from.
pub(crate) const REMOTE_URL: &str = "https://ltpp.vip/euv";

/// The base URL for resolving relative resource paths.
pub(crate) const REMOTE_BASE_URL: &str = "https://ltpp.vip/static/euv/";

/// Top-level subdirectory under app_cache_dir for all cache data.
pub(crate) const CACHE_DIR: &str = "euv_web_cache";

/// The symlink/file name that points to the currently active cache version.
pub(crate) const ACTIVE_LINK: &str = "active";

/// Prefix for versioned cache directories (e.g., "v_1718000000123").
pub(crate) const VERSION_PREFIX: &str = "v_";

/// Timeout in seconds for remote fetch requests.
pub(crate) const FETCH_TIMEOUT_SECS: u64 = 30;

/// Maximum response body size in bytes (10 MB).
pub(crate) const MAX_BODY_SIZE: usize = 10485760;

/// Retry interval in milliseconds when network fetch fails.
pub(crate) const RETRY_INTERVAL_MILLIS: u64 = 1000;

/// Maximum number of HTTP redirects to follow.
pub(crate) const MAX_REDIRECTS: usize = 10;

/// The custom URI scheme name for serving cached resources.
pub(crate) const SCHEME_NAME: &str = "euv";

/// Maximum number of old cache versions to keep (for rollback).
pub(crate) const MAX_KEPT_VERSIONS: usize = 2;
