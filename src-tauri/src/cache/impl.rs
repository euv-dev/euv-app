use super::*;

/// Implements `std::fmt::Display` for `CacheError`.
impl std::fmt::Display for CacheError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CacheError::Fetch(message) => write!(f, "Fetch: {message}"),
            CacheError::Write(message) => write!(f, "Write: {message}"),
            CacheError::Read(message) => write!(f, "Read: {message}"),
        }
    }
}

/// Implements `std::error::Error` for `CacheError`.
impl std::error::Error for CacheError {}

impl CacheUpdateStatus {
    /// String tag sent over the bridge.
    ///
    /// Maps the enum to the lowercase literal that the JS side compares
    /// against — see `UPDATE_RESULT_SUCCESS` / `UPDATE_RESULT_FAILED` in
    /// `super::const`. Centralising the mapping here keeps the wire format
    /// a single grep target.
    ///
    /// # Returns
    ///
    /// - `&'static str`: The lowercase status tag for this variant.
    fn as_tag(self) -> &'static str {
        match self {
            CacheUpdateStatus::Success => UPDATE_RESULT_SUCCESS,
            CacheUpdateStatus::Failed => UPDATE_RESULT_FAILED,
        }
    }
}

/// Implements `Serialize` for `CacheUpdateStatus`.
///
/// Emits the lowercase tag from `as_tag` so the JS side can compare with
/// `===` directly — see the matching `UPDATE_RESULT_*` constants on the
/// webview side.
impl Serialize for CacheUpdateStatus {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_tag())
    }
}
