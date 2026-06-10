/// The result type for fetching a list of resources: a vector of
/// (clean_path, final_url, data) tuples.
///
/// `final_url` is the URL of the resource AFTER following all redirects, which is
/// required to correctly resolve transitive dependencies (e.g. a `.wasm` file
/// referenced from a redirected `.js` file).
pub(crate) type FetchResult = Vec<(String, String, Vec<u8>)>;
