use crate::*;

pub(crate) static SERVING_VERSION: OnceLock<Mutex<String>> = OnceLock::new();
