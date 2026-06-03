use crate::*;

pub(crate) static SERVING_VERSION: OnceLock<Mutex<String>> = OnceLock::new();
pub(crate) static SERVING_SOURCE: OnceLock<Mutex<String>> = OnceLock::new();
#[cfg(debug_assertions)]
pub(crate) static APP_HANDLE: OnceLock<AppHandle> = OnceLock::new();
