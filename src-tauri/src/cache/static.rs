use crate::*;

#[cfg(debug_assertions)]
pub(crate) static APP_HANDLE: OnceLock<AppHandle> = OnceLock::new();
