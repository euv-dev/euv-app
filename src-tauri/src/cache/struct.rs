use serde::Serialize;
use std::sync::OnceLock;
use std::sync::Mutex;

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

/// The version that the scheme handler is currently serving.
/// Once set for a process lifetime, the scheme handler always reads from this version directory.
/// This prevents mid-load version switches from causing white screens.
static SERVING_VERSION: OnceLock<Mutex<String>> = OnceLock::new();

pub(crate) fn get_serving_version() -> Option<String> {
    SERVING_VERSION.get().and_then(|m| {
        m.lock().ok().map(|s| s.clone())
    }).filter(|s| !s.is_empty())
}

pub(crate) fn set_serving_version(version: &str) {
    match SERVING_VERSION.get() {
        Some(m) => {
            if let Ok(mut s) = m.lock() {
                *s = version.to_string();
            }
        }
        None => {
            let _ = SERVING_VERSION.set(Mutex::new(version.to_string()));
        }
    }
}
