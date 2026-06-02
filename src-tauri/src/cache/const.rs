include!(concat!(env!("OUT_DIR"), "/config_generated.rs"));
pub(crate) const ACTIVE_LINK: &str = "active";
pub(crate) const VERSION_PREFIX: &str = "v_";
pub(crate) const FETCH_TIMEOUT_SECS: u64 = 30;
pub(crate) const MAX_BODY_SIZE: usize = 10485760;
pub(crate) const RETRY_INTERVAL_MILLIS: u64 = 1000;
pub(crate) const SCHEME_NAME: &str = "euv";
pub(crate) const MAX_KEPT_VERSIONS: usize = 2;
