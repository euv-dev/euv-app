use crate::*;

// ─── Directory & Path Helpers ────────────────────────────────────────────────

/// Gets the top-level cache directory (contains versioned subdirs and the active pointer).
pub(crate) fn get_cache_root(app_handle: &tauri::AppHandle) -> Result<PathBuf, CacheError> {
    let mut dir: PathBuf = app_handle
        .path()
        .app_cache_dir()
        .map_err(|e| CacheError::Read(format!("{}", e)))?;
    dir.push(CACHE_DIR);
    Ok(dir)
}

/// Returns the path to the "active" pointer file.
/// This file contains the name of the currently active versioned directory.
fn active_pointer_path(cache_root: &std::path::Path) -> PathBuf {
    cache_root.join(ACTIVE_LINK)
}

/// Reads the active version directory name from the pointer file.
/// Returns None if no active version exists.
fn read_active_version(cache_root: &std::path::Path) -> Option<String> {
    let pointer: PathBuf = active_pointer_path(cache_root);
    let version_name: String = std::fs::read_to_string(&pointer).ok()?;
    let version_name: &str = version_name.trim();
    if version_name.is_empty() {
        return None;
    }
    // Verify the directory actually exists
    let version_dir: PathBuf = cache_root.join(version_name);
    if version_dir.is_dir() {
        Some(version_name.to_string())
    } else {
        None
    }
}

/// Returns the path to the currently active cache directory.
/// Returns None if no active version is set.
fn get_active_cache_dir(cache_root: &std::path::Path) -> Option<PathBuf> {
    let version_name: String = read_active_version(cache_root)?;
    Some(cache_root.join(version_name))
}

/// Generates a new version directory name based on the current timestamp.
fn new_version_name() -> String {
    let ts: u128 = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    format!("{}{}", VERSION_PREFIX, ts)
}

/// Atomically switches the active pointer to a new version directory.
/// Uses write-to-temp + rename for atomic switch on the pointer file itself.
fn switch_active_version(
    cache_root: &std::path::Path,
    version_name: &str,
) -> Result<(), CacheError> {
    let pointer: PathBuf = active_pointer_path(cache_root);
    let tmp_pointer: PathBuf = cache_root.join(format!(".active_tmp_{}", std::process::id()));

    // Write new version name to temp file
    std::fs::write(&tmp_pointer, version_name)
        .map_err(|e| CacheError::Write(format!("write active pointer tmp: {}", e)))?;

    // Atomic rename (on same filesystem this is atomic on POSIX and Windows)
    std::fs::rename(&tmp_pointer, &pointer)
        .map_err(|e| CacheError::Write(format!("rename active pointer: {}", e)))?;

    log::info!("[EUV] switched active cache to: {}", version_name);
    Ok(())
}

/// Removes old cache versions, keeping only the most recent MAX_KEPT_VERSIONS.
fn cleanup_old_versions(cache_root: &std::path::Path, current_version: &str) {
    let entries: Vec<std::fs::DirEntry> = match std::fs::read_dir(cache_root) {
        Ok(rd) => rd.filter_map(|e| e.ok()).collect(),
        Err(_) => return,
    };

    // Collect versioned directories, sorted by name (timestamp-based, oldest first)
    let mut versions: Vec<String> = entries
        .iter()
        .filter_map(|e| {
            let name: String = e.file_name().to_string_lossy().to_string();
            if name.starts_with(VERSION_PREFIX) && e.path().is_dir() && name != current_version {
                Some(name)
            } else {
                None
            }
        })
        .collect();
    versions.sort();

    // Keep at most MAX_KEPT_VERSIONS (including the current one)
    // So we remove everything beyond MAX_KEPT_VERSIONS - 1 old versions
    let max_old: usize = MAX_KEPT_VERSIONS.saturating_sub(1);
    if versions.len() <= max_old {
        return;
    }
    let to_remove: usize = versions.len() - max_old;
    for name in &versions[..to_remove] {
        let path: PathBuf = cache_root.join(name);
        if let Err(e) = std::fs::remove_dir_all(&path) {
            log::warn!("[EUV] failed to remove old cache version {}: {}", name, e);
        } else {
            log::info!("[EUV] removed old cache version: {}", name);
        }
    }
}

// ─── Network Fetch ───────────────────────────────────────────────────────────

/// Fetches a remote URL and returns (content_bytes, content_type, final_url).
/// Decompression (gzip/br/deflate) is handled automatically by reqwest via feature flags.
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

// ─── Atomic File Write ───────────────────────────────────────────────────────

/// Writes data to a file atomically: write to a temp file in the same directory,
/// then rename over the target. This prevents partial/corrupted files.
fn atomic_write(target: &std::path::Path, data: &[u8]) -> Result<(), CacheError> {
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| CacheError::Write(format!("mkdir: {}", e)))?;
    }
    let tmp_path: PathBuf = target.with_extension("tmp");
    std::fs::write(&tmp_path, data)
        .map_err(|e| CacheError::Write(format!("write tmp: {}", e)))?;
    std::fs::rename(&tmp_path, target)
        .map_err(|e| CacheError::Write(format!("rename: {}", e)))?;
    Ok(())
}

// ─── Cache Loading ───────────────────────────────────────────────────────────

/// Loads the cached index.html from the currently active cache directory.
pub(crate) fn load_cached_html(app_handle: &tauri::AppHandle) -> Option<String> {
    let cache_root: PathBuf = get_cache_root(app_handle).ok()?;
    let active_dir: PathBuf = get_active_cache_dir(&cache_root)?;
    let index_path: PathBuf = active_dir.join("index.html");
    if index_path.exists() {
        let html: String = std::fs::read_to_string(&index_path).ok()?;
        if !html.is_empty() {
            return Some(html);
        }
    }
    None
}

/// Populates the runtime cache from bundled (embedded) resources.
/// Creates a new version directory and switches active pointer to it.
/// Returns true if bundled resources were available and deployed.
pub(crate) fn deploy_bundled_cache(app_handle: &tauri::AppHandle) -> bool {
    let bundled_files = crate::cache::bundled::BUNDLED_FILES;
    if bundled_files.is_empty() {
        log::info!("[EUV] no bundled cache resources available");
        return false;
    }
    let cache_root: PathBuf = match get_cache_root(app_handle) {
        Ok(dir) => dir,
        Err(e) => {
            log::error!("[EUV] failed to get cache root for bundled deploy: {}", e);
            return false;
        }
    };
    if let Err(e) = std::fs::create_dir_all(&cache_root) {
        log::error!("[EUV] failed to create cache root: {}", e);
        return false;
    }

    let version_name: String = new_version_name();
    let version_dir: PathBuf = cache_root.join(&version_name);
    if let Err(e) = std::fs::create_dir_all(&version_dir) {
        log::error!("[EUV] failed to create version dir: {}", e);
        return false;
    }

    let mut count: usize = 0;
    for (rel_path, data) in bundled_files {
        let target_path: PathBuf = version_dir.join(rel_path);
        if let Some(parent) = target_path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        if let Err(e) = std::fs::write(&target_path, data) {
            log::warn!("[EUV] failed to write bundled file {}: {}", rel_path, e);
        } else {
            count += 1;
        }
    }

    if count == 0 {
        // No files written, remove the empty version dir
        std::fs::remove_dir_all(&version_dir).ok();
        return false;
    }

    // Switch active pointer to this new version
    if let Err(e) = switch_active_version(&cache_root, &version_name) {
        log::error!("[EUV] failed to switch to bundled version: {}", e);
        return false;
    }

    log::info!("[EUV] deployed {} bundled cache files", count);
    true
}

// ─── Fetching & Updating ─────────────────────────────────────────────────────

/// Fetches all resources into a NEW version directory, then atomically switches
/// the active pointer. The old version remains intact while the new one is being built,
/// eliminating any race condition between the frontend reading and the backend writing.
async fn fetch_full_snapshot(cache_root: &std::path::Path) -> Result<String, CacheError> {
    // 1. Fetch the HTML
    let (html_bytes, _content_type, _final_url): (Vec<u8>, String, String) =
        fetch_url(REMOTE_URL).await?;
    let html: String = String::from_utf8_lossy(&html_bytes).to_string();
    if html.is_empty() {
        return Err(CacheError::Fetch("empty HTML response".to_string()));
    }

    // 2. Create a new version directory
    let version_name: String = new_version_name();
    let version_dir: PathBuf = cache_root.join(&version_name);
    std::fs::create_dir_all(&version_dir)
        .map_err(|e| CacheError::Write(format!("mkdir version dir: {}", e)))?;

    // 3. Write HTML atomically
    let index_path: PathBuf = version_dir.join("index.html");
    atomic_write(&index_path, html.as_bytes())?;

    // 4. Fetch all linked resources into the new version directory
    let fetch_errors: usize = fetch_linked_resources(&version_dir, &html).await;
    if fetch_errors > 0 {
        log::warn!(
            "[EUV] {} resource(s) failed to fetch for version {}",
            fetch_errors,
            version_name
        );
    }

    // 5. Atomically switch active pointer to the new version
    switch_active_version(cache_root, &version_name)?;

    // 6. Clean up old versions in the background
    cleanup_old_versions(cache_root, &version_name);

    Ok(version_name)
}

/// Initial fetch when no cache exists. Fetches HTML with retry, builds a complete
/// snapshot, and then switches the active pointer.
pub(crate) async fn initial_fetch_and_notify(app_handle: tauri::AppHandle) {
    log::info!("[EUV] initial fetch started (no cache)");
    let cache_root: PathBuf = match get_cache_root(&app_handle) {
        Ok(dir) => dir,
        Err(e) => {
            log::error!("[EUV] initial fetch cache root error: {}", e);
            return;
        }
    };
    std::fs::create_dir_all(&cache_root).ok();

    loop {
        match fetch_full_snapshot(&cache_root).await {
            Ok(version) => {
                log::info!("[EUV] initial fetch complete, active version: {}", version);
                return;
            }
            Err(e) => {
                log::warn!(
                    "[EUV] initial fetch failed: {}, retrying in {}ms",
                    e,
                    RETRY_INTERVAL_MILLIS
                );
                tokio::time::sleep(std::time::Duration::from_millis(RETRY_INTERVAL_MILLIS)).await;
            }
        }
    }
}

/// Background task: silently updates cache when cache already exists.
/// Builds a complete new snapshot in a separate directory, then atomically switches.
/// The currently active version is NEVER modified — frontend reads are always safe.
pub(crate) async fn update_cache_async(app_handle: tauri::AppHandle) {
    log::info!("[EUV] background cache update started");
    let cache_root: PathBuf = match get_cache_root(&app_handle) {
        Ok(dir) => dir,
        Err(e) => {
            log::error!("[EUV] background cache root error: {}", e);
            return;
        }
    };
    std::fs::create_dir_all(&cache_root).ok();

    match fetch_full_snapshot(&cache_root).await {
        Ok(version) => {
            log::info!("[EUV] background cache update done, new version: {}", version);
        }
        Err(e) => {
            log::warn!(
                "[EUV] background cache update failed (will retry next launch): {}",
                e
            );
        }
    }
}

// ─── Linked Resources ────────────────────────────────────────────────────────

/// Fetches resources linked in the HTML (script, link, img sources) and writes them
/// into the given version directory. Returns the number of failed fetches.
async fn fetch_linked_resources(version_dir: &std::path::Path, html: &str) -> usize {
    let mut resource_paths: Vec<String> = Vec::new();
    extract_attr_values(html, "script", "src", &mut resource_paths);
    extract_attr_values(html, "link", "href", &mut resource_paths);
    extract_attr_values(html, "img", "src", &mut resource_paths);
    extract_module_imports(html, &mut resource_paths);

    let base_url: String = REMOTE_BASE_URL.trim_end_matches('/').to_string();
    let mut errors: usize = 0;

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
        let local_path: PathBuf = version_dir.join(clean_path);

        match fetch_url(&remote_url).await {
            Ok((data, _ct, _final_url)) => {
                if let Err(e) = atomic_write(&local_path, &data) {
                    log::warn!("[EUV] failed to write resource {}: {}", clean_path, e);
                    errors += 1;
                } else {
                    log::info!("[EUV] cached resource: {}", clean_path);
                }
            }
            Err(e) => {
                log::warn!("[EUV] failed to fetch resource {}: {}", clean_path, e);
                errors += 1;
            }
        }
    }
    errors
}

// ─── HTML Parsing ────────────────────────────────────────────────────────────

/// Extracts ES module import paths from HTML script content.
fn extract_module_imports(html: &str, results: &mut Vec<String>) {
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
            if !value.is_empty() && !results.contains(&value.to_string()) {
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

// ─── URI Scheme Handler ──────────────────────────────────────────────────────

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
/// Reads cached resources from the currently ACTIVE version directory.
/// Since the active directory is never mutated in-place, reads are always consistent.
pub(crate) fn handle_euv_scheme(
    app_handle: &tauri::AppHandle,
    request: http::Request<Vec<u8>>,
) -> http::Response<Cow<'static, [u8]>> {
    let uri: &http::uri::Uri = request.uri();
    let path: &str = uri.path();
    log::info!(
        "[EUV] handle_euv_scheme called: method={} uri={}",
        request.method(),
        uri
    );
    let path_trimmed: &str = path.trim_start_matches('/');
    let path_decoded: String = percent_encoding::percent_decode_str(path_trimmed)
        .decode_utf8_lossy()
        .into_owned();

    let cache_root: PathBuf = match get_cache_root(app_handle) {
        Ok(dir) => dir,
        Err(e) => {
            log::error!("[EUV] failed to get cache root: {}", e);
            return http::Response::builder()
                .status(http::StatusCode::INTERNAL_SERVER_ERROR)
                .body(cow_owned(b"Internal error".to_vec()))
                .unwrap();
        }
    };

    // Read the active version — this is the ONLY directory we ever serve from
    let active_dir: PathBuf = match get_active_cache_dir(&cache_root) {
        Some(dir) => dir,
        None => {
            log::error!("[EUV] no active cache version available");
            return http::Response::builder()
                .status(http::StatusCode::SERVICE_UNAVAILABLE)
                .body(cow_owned(b"Cache not ready".to_vec()))
                .unwrap();
        }
    };

    let file_path: PathBuf = if path_decoded.is_empty() || path_decoded == "index.html" {
        active_dir.join("index.html")
    } else {
        active_dir.join(&path_decoded)
    };

    log::info!(
        "[EUV] handle_euv_scheme: looking for file at {:?}",
        file_path
    );
    match std::fs::read(&file_path) {
        Ok(data) => {
            let mime: &str = mime_type_for_path(&path_decoded);
            log::info!(
                "[EUV] handle_euv_scheme: serving {} ({} bytes, mime={})",
                path_decoded,
                data.len(),
                mime
            );
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

// ─── Tauri Command ───────────────────────────────────────────────────────────

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
