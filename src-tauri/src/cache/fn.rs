use crate::*;

/// Gets the cache directory path under the app's cache directory.
pub(crate) fn get_cache_dir(app_handle: &tauri::AppHandle) -> Result<PathBuf, CacheError> {
    let mut dir: PathBuf = app_handle
        .path()
        .app_cache_dir()
        .map_err(|e| CacheError::Read(format!("{}", e)))?;
    dir.push(CACHE_DIR);
    Ok(dir)
}

/// Fetches a remote URL and returns (content_bytes, content_type, final_url).
pub(crate) async fn fetch_url(url: &str) -> Result<(Vec<u8>, String, String), CacheError> {
    let client: reqwest::Client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(FETCH_TIMEOUT_SECS))
        .redirect(reqwest::redirect::Policy::limited(MAX_REDIRECTS))
        .build()
        .map_err(|e| CacheError::Fetch(e.to_string()))?;
    let resp: reqwest::Response = client
        .get(url)
        .send()
        .await
        .map_err(|e| CacheError::Fetch(e.to_string()))?;
    let final_url: String = resp.url().to_string();
    let status: reqwest::StatusCode = resp.status();
    if !status.is_success() {
        return Err(CacheError::Fetch(format!("HTTP {}", status)));
    }
    let content_type: String = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/octet-stream")
        .to_string();
    let bytes: Vec<u8> = resp
        .bytes()
        .await
        .map_err(|e| CacheError::Fetch(e.to_string()))?
        .to_vec();
    if bytes.len() > MAX_BODY_SIZE {
        return Err(CacheError::Fetch(format!(
            "Too large: {} bytes",
            bytes.len()
        )));
    }
    Ok((bytes, content_type, final_url))
}

/// Fetches the main page HTML from the remote URL and saves it to cache.
pub(crate) async fn fetch_and_save_html(
    app_handle: &tauri::AppHandle,
) -> Result<String, CacheError> {
    let cache_dir: PathBuf = get_cache_dir(app_handle)?;
    std::fs::create_dir_all(&cache_dir).map_err(|e| CacheError::Write(e.to_string()))?;
    let (html_bytes, _content_type, _final_url): (Vec<u8>, String, String) =
        fetch_url(REMOTE_URL).await?;
    let html: String = String::from_utf8_lossy(&html_bytes).to_string();
    let index_path: PathBuf = cache_dir.join("index.html");
    std::fs::write(&index_path, &html).map_err(|e| CacheError::Write(e.to_string()))?;
    Ok(html)
}

/// Fetches the main page HTML with retry logic. Retries every RETRY_INTERVAL_MILLIS
/// until successful.
pub(crate) async fn fetch_and_save_html_with_retry(
    app_handle: &tauri::AppHandle,
) -> Result<String, CacheError> {
    loop {
        match fetch_and_save_html(app_handle).await {
            Ok(html) => return Ok(html),
            Err(e) => {
                log::warn!(
                    "[EUV] fetch failed: {}, retrying in {}ms",
                    e,
                    RETRY_INTERVAL_MILLIS
                );
                tokio::time::sleep(std::time::Duration::from_millis(RETRY_INTERVAL_MILLIS)).await;
            }
        }
    }
}

/// Loads the cached index.html from the local cache directory.
pub(crate) fn load_cached_html(app_handle: &tauri::AppHandle) -> Option<String> {
    let cache_dir: PathBuf = get_cache_dir(app_handle).ok()?;
    let index_path: PathBuf = cache_dir.join("index.html");
    if index_path.exists() {
        let html: String = std::fs::read_to_string(&index_path).ok()?;
        if !html.is_empty() {
            return Some(html);
        }
    }
    None
}

/// Populates the runtime cache from bundled (embedded) resources.
/// Returns true if bundled resources were available and deployed.
pub(crate) fn deploy_bundled_cache(app_handle: &tauri::AppHandle) -> bool {
    let bundled_files = crate::cache::bundled::BUNDLED_FILES;
    if bundled_files.is_empty() {
        log::info!("[EUV] no bundled cache resources available");
        return false;
    }
    let cache_dir: PathBuf = match get_cache_dir(app_handle) {
        Ok(dir) => dir,
        Err(e) => {
            log::error!("[EUV] failed to get cache dir for bundled deploy: {}", e);
            return false;
        }
    };
    if let Err(e) = std::fs::create_dir_all(&cache_dir) {
        log::error!("[EUV] failed to create cache dir: {}", e);
        return false;
    }
    let mut count: usize = 0;
    for (rel_path, data) in bundled_files {
        let target_path: PathBuf = cache_dir.join(rel_path);
        if let Some(parent) = target_path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        if let Err(e) = std::fs::write(&target_path, data) {
            log::warn!("[EUV] failed to write bundled file {}: {}", rel_path, e);
        } else {
            count += 1;
        }
    }
    log::info!("[EUV] deployed {} bundled cache files", count);
    count > 0
}

/// Determines the MIME type based on file extension.
pub(crate) fn mime_type_for_path(path: &str) -> &'static str {
    let lower: &str = &path.to_lowercase();
    if lower.ends_with(".html") || lower.ends_with(".htm") {
        "text/html"
    } else if lower.ends_with(".js") || lower.ends_with(".mjs") {
        "application/javascript"
    } else if lower.ends_with(".wasm") {
        "application/wasm"
    } else if lower.ends_with(".css") {
        "text/css"
    } else if lower.ends_with(".json") {
        "application/json"
    } else if lower.ends_with(".png") {
        "image/png"
    } else if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
        "image/jpeg"
    } else if lower.ends_with(".gif") {
        "image/gif"
    } else if lower.ends_with(".svg") {
        "image/svg+xml"
    } else if lower.ends_with(".ico") {
        "image/x-icon"
    } else if lower.ends_with(".woff") {
        "font/woff"
    } else if lower.ends_with(".woff2") {
        "font/woff2"
    } else if lower.ends_with(".ttf") {
        "font/ttf"
    } else if lower.ends_with(".otf") {
        "font/otf"
    } else if lower.ends_with(".webp") {
        "image/webp"
    } else {
        "application/octet-stream"
    }
}

/// Converts an owned Vec<u8> to Cow<'static, [u8]>.
fn cow_owned(data: Vec<u8>) -> Cow<'static, [u8]> {
    Cow::Owned(data)
}

/// Handles requests for the custom `euv://` URI scheme.
/// Reads cached resources from the local file system.
pub(crate) fn handle_euv_scheme(
    app_handle: &tauri::AppHandle,
    request: http::Request<Vec<u8>>,
) -> http::Response<Cow<'static, [u8]>> {
    let uri: &http::uri::Uri = request.uri();
    let path: &str = uri.path();
    log::info!("[EUV] handle_euv_scheme called: method={} uri={}", request.method(), uri);
    let path_trimmed: &str = path.trim_start_matches('/');
    let path_decoded: String = percent_encoding::percent_decode_str(path_trimmed)
        .decode_utf8_lossy()
        .into_owned();
    let cache_dir: PathBuf = match get_cache_dir(app_handle) {
        Ok(dir) => dir,
        Err(e) => {
            log::error!("[EUV] failed to get cache dir: {}", e);
            return http::Response::builder()
                .status(http::StatusCode::INTERNAL_SERVER_ERROR)
                .body(cow_owned(b"Internal error".to_vec()))
                .unwrap();
        }
    };
    let file_path: PathBuf = if path_decoded.is_empty() || path_decoded == "index.html" {
        cache_dir.join("index.html")
    } else {
        cache_dir.join(&path_decoded)
    };
    log::info!("[EUV] handle_euv_scheme: looking for file at {:?}", file_path);
    match std::fs::read(&file_path) {
        Ok(data) => {
            let mime: &str = mime_type_for_path(&path_decoded);
            log::info!("[EUV] handle_euv_scheme: serving {} ({} bytes, mime={})", path_decoded, data.len(), mime);
            let mut builder: http::response::Builder = http::Response::builder()
                .status(http::StatusCode::OK)
                .header("Content-Type", mime)
                .header("Cache-Control", "no-cache")
                .header("Access-Control-Allow-Origin", "*");
            if path_decoded.ends_with(".wasm") {
                builder = builder.header("Cross-Origin-Embedder-Policy", "require-corp");
                builder = builder.header("Cross-Origin-Opener-Policy", "same-origin");
            }
            builder.body(cow_owned(data)).unwrap()
        }
        Err(e) => {
            log::warn!(
                "[EUV] file not found in cache: {} ({})",
                file_path.display(),
                e
            );
            http::Response::builder()
                .status(http::StatusCode::NOT_FOUND)
                .header("Content-Type", "text/plain")
                .body(cow_owned(b"Not found".to_vec()))
                .unwrap()
        }
    }
}

/// Initial fetch when no cache exists. Fetches HTML with retry, then triggers
/// frontend navigation to load cached page. Continues to fetch all linked resources.
pub(crate) async fn initial_fetch_and_notify(app_handle: tauri::AppHandle) {
    log::info!("[EUV] initial fetch started (no cache)");
    let cache_dir: PathBuf = match get_cache_dir(&app_handle) {
        Ok(dir) => dir,
        Err(e) => {
            log::error!("[EUV] initial fetch cache dir error: {}", e);
            return;
        }
    };
    std::fs::create_dir_all(&cache_dir).ok();
    match fetch_and_save_html_with_retry(&app_handle).await {
        Ok(html) => {
            log::info!("[EUV] initial HTML cached, fetching linked resources");
            fetch_linked_resources(&cache_dir, &html, true).await;
            log::info!("[EUV] initial fetch complete");
        }
        Err(e) => {
            log::error!("[EUV] initial fetch error: {}", e);
        }
    }
}

/// Background task: silently updates cache when cache already exists.
/// Always re-fetches all resources (overwrite existing), then fetches linked resources.
pub(crate) async fn update_cache_async(app_handle: tauri::AppHandle) {
    log::info!("[EUV] background cache update started");
    let cache_dir: PathBuf = match get_cache_dir(&app_handle) {
        Ok(dir) => dir,
        Err(e) => {
            log::error!("[EUV] background cache dir error: {}", e);
            return;
        }
    };
    std::fs::create_dir_all(&cache_dir).ok();
    match fetch_and_save_html(&app_handle).await {
        Ok(html) => {
            log::info!("[EUV] background HTML cache updated, fetching linked resources");
            fetch_linked_resources(&cache_dir, &html, false).await;
            log::info!("[EUV] background cache update done");
        }
        Err(e) => {
            log::warn!(
                "[EUV] background cache update failed (will retry next launch): {}",
                e
            );
        }
    }
}

/// Fetches resources linked in the HTML (script, link, img sources).
/// When `skip_existing` is true, resources already in cache are skipped.
/// When `skip_existing` is false, all resources are re-fetched (overwrite).
async fn fetch_linked_resources(cache_dir: &std::path::Path, html: &str, skip_existing: bool) {
    let mut resource_paths: Vec<String> = Vec::new();
    extract_attr_values(html, "script", "src", &mut resource_paths);
    extract_attr_values(html, "link", "href", &mut resource_paths);
    extract_attr_values(html, "img", "src", &mut resource_paths);
    // Also extract ES module imports: import ... from './path'
    extract_module_imports(html, &mut resource_paths);
    let base_url: String = REMOTE_BASE_URL.trim_end_matches('/').to_string();
    for relative_path in resource_paths {
        if relative_path.starts_with("data:")
            || relative_path.starts_with("http://")
            || relative_path.starts_with("https://")
            || relative_path.starts_with("//")
        {
            continue;
        }
        let clean_path: &str = relative_path
            .trim_start_matches('/')
            .trim_start_matches("./");
        let remote_url: String = format!("{}/{}", base_url, clean_path);
        let local_path: PathBuf = cache_dir.join(clean_path);
        if skip_existing && local_path.exists() {
            continue;
        }
        if let Some(parent) = local_path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        match fetch_url(&remote_url).await {
            Ok((data, _ct, _final_url)) => {
                if let Err(e) = std::fs::write(&local_path, &data) {
                    log::warn!("[EUV] failed to write resource {}: {}", clean_path, e);
                } else {
                    log::info!("[EUV] cached resource: {}", clean_path);
                }
            }
            Err(e) => {
                log::warn!("[EUV] failed to fetch resource {}: {}", clean_path, e);
            }
        }
    }
}

/// Extracts ES module import paths from HTML script content.
fn extract_module_imports(html: &str, results: &mut Vec<String>) {
    // Match: import ... from './path' or import ... from "./path"
    let mut search_from: usize = 0;
    while search_from < html.len() {
        let from_pos: usize = match html[search_from..].find("from") {
            Some(pos) => search_from + pos,
            None => break,
        };
        let after_from: &str = &html[from_pos + 4..];
        let trimmed: &str = after_from.trim_start();
        let value: &str = extract_quoted_value(trimmed);
        if !value.is_empty()
            && (value.starts_with("./") || value.starts_with("../"))
            && !results.contains(&value.to_string())
        {
            results.push(value.to_string());
        }
        search_from = from_pos + 4;
    }
}

/// Extracts attribute values from HTML tags matching the given tag name and attribute.
fn extract_attr_values(html: &str, tag_name: &str, attr_name: &str, results: &mut Vec<String>) {
    let tag_pattern: String = format!("<{}", tag_name.to_lowercase());
    let attr_pattern: String = format!("{}=", attr_name.to_lowercase());
    let mut search_from: usize = 0;
    while search_from < html.len() {
        let tag_start: usize = match html[search_from..].find(&tag_pattern) {
            Some(pos) => search_from + pos,
            None => break,
        };
        let tag_end: usize = match html[tag_start..].find('>') {
            Some(pos) => tag_start + pos,
            None => break,
        };
        let tag_content: String = html[tag_start..tag_end].to_lowercase();
        if let Some(attr_pos) = tag_content.find(&attr_pattern) {
            let after_attr: &str = &tag_content[attr_pos + attr_pattern.len()..];
            let value: &str = extract_quoted_value(after_attr);
            if !value.is_empty() {
                results.push(value.to_string());
            }
        }
        search_from = tag_end + 1;
    }
}

/// Extracts a value from a quoted string (single or double quotes).
fn extract_quoted_value(s: &str) -> &str {
    let trimmed: &str = s.trim_start();
    if let Some(rest) = trimmed
        .strip_prefix('"')
        .and_then(|r| Some((r, r.find('"')?)))
    {
        let (rest, end) = rest;
        return &rest[..end];
    } else if let Some(rest) = trimmed
        .strip_prefix('\'')
        .and_then(|r| Some((r, r.find('\'')?)))
    {
        let (rest, end) = rest;
        return &rest[..end];
    }
    ""
}

/// Tauri command: checks whether a cached page is available.
/// Returns cache status and the remote URL as fallback.
#[tauri::command]
pub(crate) async fn load_cached_resource(
    app_handle: tauri::AppHandle,
) -> Result<CachedPage, String> {
    let from_cache: bool = load_cached_html(&app_handle).is_some();
    Ok(CachedPage {
        from_cache,
        remote_url: REMOTE_URL.to_string(),
    })
}
