use crate::*;

impl std::fmt::Display for CacheError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CacheError::Fetch(message) => write!(f, "Fetch: {message}"),
            CacheError::Write(message) => write!(f, "Write: {message}"),
            CacheError::Read(message) => write!(f, "Read: {message}"),
        }
    }
}

impl std::error::Error for CacheError {}
