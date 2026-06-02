use crate::cache::CacheError;

impl std::fmt::Display for CacheError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CacheError::Fetch(msg) => write!(f, "Fetch: {}", msg),
            CacheError::Write(msg) => write!(f, "Write: {}", msg),
            CacheError::Read(msg) => write!(f, "Read: {}", msg),
        }
    }
}

impl std::error::Error for CacheError {}
