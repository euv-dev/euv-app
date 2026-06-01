use crate::*;

impl std::fmt::Display for CacheError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CacheError::Fetch(msg) => write!(f, "Fetch error: {}", msg),
            CacheError::Write(msg) => write!(f, "Write error: {}", msg),
            CacheError::Read(msg) => write!(f, "Read error: {}", msg),
        }
    }
}
