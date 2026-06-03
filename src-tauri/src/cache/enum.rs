#[derive(Debug)]
pub(crate) enum CacheError {
    Fetch(String),
    Write(String),
    Read(String),
}
