use crate::*;

/// Implements Display trait for CacheError.
impl std::fmt::Display for CacheError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CacheError::Read(message) => write!(formatter, "Cache read error: {}", message),
            CacheError::Write(message) => write!(formatter, "Cache write error: {}", message),
            CacheError::Fetch(message) => write!(formatter, "Fetch error: {}", message),
        }
    }
}

/// Implements std::error::Error for CacheError.
impl std::error::Error for CacheError {}
