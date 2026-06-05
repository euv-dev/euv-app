use crate::*;

pub(crate) fn get_serving_version() -> Option<String> {
    SERVING_VERSION
        .get()
        .and_then(|mutex: &Mutex<String>| {
            mutex
                .lock()
                .ok()
                .map(|guard: std::sync::MutexGuard<String>| guard.clone())
        })
        .filter(|version: &String| !version.is_empty())
}

pub(crate) fn set_serving_version(version: &str) {
    match SERVING_VERSION.get() {
        Some(mutex) => {
            if let Ok(mut guard) = mutex.lock() {
                *guard = version.to_string();
            }
        }
        None => {
            let _ = SERVING_VERSION.set(Mutex::new(version.to_string()));
        }
    }
}

pub(crate) fn get_serving_source() -> String {
    SERVING_SOURCE
        .get()
        .and_then(|mutex: &Mutex<String>| {
            mutex
                .lock()
                .ok()
                .map(|guard: std::sync::MutexGuard<String>| guard.clone())
        })
        .unwrap_or_else(|| "unknown".to_string())
}

pub(crate) fn set_serving_source(source: &str) {
    match SERVING_SOURCE.get() {
        Some(mutex) => {
            if let Ok(mut guard) = mutex.lock() {
                *guard = source.to_string();
            }
        }
        None => {
            let _ = SERVING_SOURCE.set(Mutex::new(source.to_string()));
        }
    }
}

pub(crate) fn get_cache_root(app_handle: &AppHandle) -> Result<PathBuf, CacheError> {
    use tauri::Manager;
    let mut cache_directory: PathBuf = app_handle
        .path()
        .app_cache_dir()
        .map_err(|error| CacheError::Read(error.to_string()))?;
    cache_directory.push(CACHE_DIR);
    Ok(cache_directory)
}

fn active_pointer_path(cache_root: &Path) -> PathBuf {
    cache_root.join(ACTIVE_LINK)
}

async fn read_active_version(cache_root: &Path) -> Option<String> {
    let name: String = read_to_string(&active_pointer_path(cache_root))
        .await
        .ok()?;
    let name: String = name.trim().to_string();
    if name.is_empty() {
        return None;
    }
    if metadata(&cache_root.join(&name)).await.ok()?.is_dir() {
        Some(name)
    } else {
        None
    }
}

async fn get_active_cache_dir(cache_root: &Path) -> Option<PathBuf> {
    let name: String = read_active_version(cache_root).await?;
    Some(cache_root.join(name))
}

fn read_active_version_sync(cache_root: &Path) -> Option<String> {
    let name: String = std::fs::read_to_string(active_pointer_path(cache_root)).ok()?;
    let name: String = name.trim().to_string();
    if name.is_empty() {
        return None;
    }
    if std::fs::metadata(cache_root.join(&name)).ok()?.is_dir() {
        Some(name)
    } else {
        None
    }
}

fn new_version_name() -> String {
    let timestamp: u128 = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    format!("{VERSION_PREFIX}{timestamp}")
}

async fn switch_active_version(cache_root: &Path, version_name: &str) -> Result<(), CacheError> {
    let pointer: PathBuf = active_pointer_path(cache_root);
    let temporary: PathBuf = cache_root.join(format!(".active_tmp_{}", std::process::id()));
    write(&temporary, version_name)
        .await
        .map_err(|error: std::io::Error| CacheError::Write(error.to_string()))?;
    rename(&temporary, &pointer)
        .await
        .map_err(|error: std::io::Error| CacheError::Write(error.to_string()))?;
    euv_log!("[EUV] switched active: {}", version_name);
    Ok(())
}

async fn cleanup_old_versions(cache_root: &Path, current: &str) {
    let serving: Option<String> = get_serving_version();
    let mut read_dir_result: tokio::fs::ReadDir = match read_dir(cache_root).await {
        Ok(entries) => entries,
        Err(_) => return,
    };
    let mut versions: Vec<String> = Vec::new();
    while let Ok(Some(entry)) = read_dir_result.next_entry().await {
        let name: String = entry.file_name().to_string_lossy().to_string();
        if name.starts_with(VERSION_PREFIX) && name != current {
            if serving.as_deref() == Some(name.as_str()) {
                continue;
            }
            if let Ok(metadata) = entry.metadata().await
                && metadata.is_dir()
            {
                versions.push(name);
            }
        }
    }
    versions.sort();
    let max_old: usize = MAX_KEPT_VERSIONS.saturating_sub(1);
    if versions.len() <= max_old {
        return;
    }
    let to_remove: usize = versions.len() - max_old;
    for name in &versions[..to_remove] {
        remove_dir_all(cache_root.join(name)).await.ok();
        euv_log!("[EUV] removed old version: {}", name);
    }
}

fn build_client() -> Result<Client, CacheError> {
    Client::builder()
        .timeout(Duration::from_secs(FETCH_TIMEOUT_SECS))
        .redirect(Policy::limited(MAX_REDIRECTS))
        .build()
        .map_err(|error: reqwest::Error| CacheError::Fetch(error.to_string()))
}

pub(crate) async fn fetch_url(url: &str) -> Result<Vec<u8>, CacheError> {
    let client: Client = build_client()?;
    let response: reqwest::Response = client
        .get(url)
        .send()
        .await
        .map_err(|error: reqwest::Error| CacheError::Fetch(error.to_string()))?;
    if !response.status().is_success() {
        return Err(CacheError::Fetch(format!("HTTP {}", response.status())));
    }
    let bytes: Vec<u8> = response
        .bytes()
        .await
        .map_err(|error: reqwest::Error| CacheError::Fetch(error.to_string()))?
        .to_vec();
    if bytes.len() > MAX_BODY_SIZE {
        return Err(CacheError::Fetch(format!(
            "Too large: {} bytes",
            bytes.len()
        )));
    }
    Ok(bytes)
}

/// Fetch a URL and return (final_url_after_redirects, body_bytes).
async fn fetch_url_with_final_url(url: &str) -> Result<(String, Vec<u8>), CacheError> {
    let client: Client = build_client()?;
    let response: reqwest::Response = client
        .get(url)
        .send()
        .await
        .map_err(|error: reqwest::Error| CacheError::Fetch(error.to_string()))?;
    if !response.status().is_success() {
        return Err(CacheError::Fetch(format!("HTTP {}", response.status())));
    }
    let final_url: String = response.url().to_string();
    let bytes: Vec<u8> = response
        .bytes()
        .await
        .map_err(|error: reqwest::Error| CacheError::Fetch(error.to_string()))?
        .to_vec();
    if bytes.len() > MAX_BODY_SIZE {
        return Err(CacheError::Fetch(format!(
            "Too large: {} bytes",
            bytes.len()
        )));
    }
    Ok((final_url, bytes))
}

async fn atomic_write(target: &Path, data: &[u8]) -> Result<(), CacheError> {
    if let Some(parent) = target.parent() {
        create_dir_all(parent)
            .await
            .map_err(|error: std::io::Error| CacheError::Write(error.to_string()))?;
    }
    let temporary: PathBuf = target.with_extension("tmp");
    write(&temporary, data)
        .await
        .map_err(|error: std::io::Error| CacheError::Write(error.to_string()))?;
    rename(&temporary, target)
        .await
        .map_err(|error: std::io::Error| CacheError::Write(error.to_string()))?;
    Ok(())
}

pub(crate) async fn load_cached_html(app_handle: &AppHandle) -> Option<String> {
    let cache_root: PathBuf = get_cache_root(app_handle).ok()?;
    let active_dir: PathBuf = get_active_cache_dir(&cache_root).await?;
    let index_path: PathBuf = active_dir.join("index.html");
    let html: String = read_to_string(&index_path).await.ok()?;
    if html.is_empty() { None } else { Some(html) }
}

#[allow(clippy::const_is_empty)]
pub(crate) async fn deploy_bundled_cache(app_handle: &AppHandle) -> bool {
    if BUNDLED_FILES.is_empty() {
        return false;
    }
    let cache_root: PathBuf = match get_cache_root(app_handle) {
        Ok(directory) => directory,
        Err(_) => return false,
    };
    if create_dir_all(&cache_root).await.is_err() {
        return false;
    }
    let version_name: String = new_version_name();
    let version_dir: PathBuf = cache_root.join(&version_name);
    if create_dir_all(&version_dir).await.is_err() {
        return false;
    }
    let mut count: usize = 0;
    for (rel_path, data) in BUNDLED_FILES {
        let target: PathBuf = version_dir.join(rel_path);
        if let Some(parent) = target.parent() {
            create_dir_all(parent).await.ok();
        }
        if write(&target, data).await.is_ok() {
            count += 1;
        }
    }
    if count == 0 {
        remove_dir_all(&version_dir).await.ok();
        return false;
    }
    if switch_active_version(&cache_root, &version_name)
        .await
        .is_err()
    {
        return false;
    }
    set_serving_source("bundled");
    euv_log!("[EUV] deployed {} bundled files", count);
    true
}

async fn fetch_full_snapshot(cache_root: &Path) -> Result<String, CacheError> {
    let (final_url, html_bytes) = fetch_url_with_final_url(REMOTE_URL).await?;
    let html: String = String::from_utf8_lossy(&html_bytes).to_string();
    if html.is_empty() {
        return Err(CacheError::Fetch("empty HTML".to_string()));
    }
    euv_log!("[EUV] final URL after redirects: {}", final_url);
    let version_name: String = new_version_name();
    let version_dir: PathBuf = cache_root.join(&version_name);
    create_dir_all(&version_dir)
        .await
        .map_err(|error: std::io::Error| CacheError::Write(error.to_string()))?;
    atomic_write(&version_dir.join("index.html"), html.as_bytes()).await?;
    let resource_count: usize = fetch_linked_resources(&version_dir, &html, &final_url).await;

    // Verify completeness: HTML must reference at least one resource, and we must have fetched them all
    let mut expected_paths: Vec<String> = Vec::new();
    extract_attr_values(&html, "script", "src", &mut expected_paths);
    extract_attr_values(&html, "link", "href", &mut expected_paths);
    extract_attr_values(&html, "img", "src", &mut expected_paths);
    extract_module_imports(&html, &mut expected_paths);
    // Filter out absolute URLs
    expected_paths.retain(|p| {
        !p.starts_with("http://")
            && !p.starts_with("https://")
            && !p.starts_with("//")
            && !p.starts_with("data:")
    });

    if resource_count == 0 && !expected_paths.is_empty() {
        // HTML references resources but none were fetched — incomplete snapshot
        remove_dir_all(&version_dir).await.ok();
        return Err(CacheError::Fetch(
            "incomplete snapshot: no resources fetched".to_string(),
        ));
    }

    // Verify critical files exist on disk (JS, CSS, images)
    for expected in &expected_paths {
        let clean: String = expected
            .trim_start_matches('/')
            .trim_start_matches("./")
            .to_string();
        let file_path: PathBuf = version_dir.join(&clean);
        if !file_path.exists() {
            euv_log!("[EUV] missing critical resource: {}", clean);
            remove_dir_all(&version_dir).await.ok();
            return Err(CacheError::Fetch(format!(
                "incomplete snapshot: missing {}",
                clean
            )));
        }
    }

    // Also verify any .wasm files discovered from JS are present.
    // Scan ALL JS/MJS files in the version directory (not just those in expected_paths)
    // because JS modules may be imported inline (e.g. `import ... from './pkg/euv.js'`)
    let mut wasm_paths: Vec<String> = Vec::new();
    // First, collect JS paths from expected_paths
    let mut js_to_scan: Vec<String> = expected_paths
        .iter()
        .map(|p| {
            p.trim_start_matches('/')
                .trim_start_matches("./")
                .to_string()
        })
        .filter(|c| c.ends_with(".js") || c.ends_with(".mjs"))
        .collect();
    // Also scan any JS files that were fetched (they may have been found via import statements)
    let pkg_dir: PathBuf = version_dir.join("pkg");
    if pkg_dir.exists() {
        if let Ok(mut entries) = tokio::fs::read_dir(&pkg_dir).await {
            while let Ok(Some(entry)) = entries.next_entry().await {
                let name: String = entry.file_name().to_string_lossy().to_string();
                if name.ends_with(".js") || name.ends_with(".mjs") {
                    let rel: String = format!("pkg/{}", name);
                    if !js_to_scan.contains(&rel) {
                        js_to_scan.push(rel);
                    }
                }
            }
        }
    }
    for js_rel in &js_to_scan {
        let js_path: PathBuf = version_dir.join(js_rel);
        if let Ok(js_content) = read_to_string(&js_path).await {
            extract_wasm_references(&js_content, js_rel, &mut wasm_paths);
        }
    }
    for wasm_ref in &wasm_paths {
        let clean: String = wasm_ref
            .trim_start_matches('/')
            .trim_start_matches("./")
            .to_string();
        let file_path: PathBuf = version_dir.join(&clean);
        if !file_path.exists() {
            euv_log!("[EUV] missing critical WASM resource: {}", clean);
            remove_dir_all(&version_dir).await.ok();
            return Err(CacheError::Fetch(format!(
                "incomplete snapshot: missing WASM {}",
                clean
            )));
        }
    }

    euv_log!(
        "[EUV] snapshot complete: {} resources fetched, all critical files verified",
        resource_count
    );
    switch_active_version(cache_root, &version_name).await?;
    cleanup_old_versions(cache_root, &version_name).await;
    Ok(version_name)
}

pub(crate) async fn initial_fetch_and_notify(app_handle: AppHandle) {
    euv_log!("[EUV] initial fetch started");
    let cache_root: PathBuf = match get_cache_root(&app_handle) {
        Ok(directory) => directory,
        Err(_) => return,
    };
    create_dir_all(&cache_root).await.ok();
    loop {
        match fetch_full_snapshot(&cache_root).await {
            Ok(version) => {
                euv_log!("[EUV] initial fetch done: {}", version);
                set_serving_version(&version);
                set_serving_source("fetched");
                euv_log!("[EUV] serving version updated to: {}", version);
                notify_reload(&app_handle);
                return;
            }
            Err(error) => {
                euv_log!("[EUV] initial fetch failed: {}, retrying", error);
                tokio::time::sleep(Duration::from_millis(RETRY_INTERVAL_MILLIS)).await;
            }
        }
    }
}

pub(crate) async fn update_cache_async(app_handle: AppHandle) {
    euv_log!("[EUV] background update started");
    let cache_root: PathBuf = match get_cache_root(&app_handle) {
        Ok(directory) => directory,
        Err(_) => return,
    };
    create_dir_all(&cache_root).await.ok();

    // Read the current serving version's HTML to compare after update
    let old_html: Option<String> = if let Some(old_version) = get_serving_version() {
        read_to_string(cache_root.join(&old_version).join("index.html"))
            .await
            .ok()
    } else {
        None
    };

    match fetch_full_snapshot(&cache_root).await {
        Ok(version) => {
            euv_log!("[EUV] background update done: {}", version);
            set_serving_version(&version);
            set_serving_source("fetched");
            euv_log!("[EUV] serving version updated to: {}", version);

            // Only reload if content actually changed
            let new_html: Option<String> =
                read_to_string(cache_root.join(&version).join("index.html"))
                    .await
                    .ok();
            let content_changed: bool = match (&old_html, &new_html) {
                (Some(old), Some(new)) => old != new,
                _ => true,
            };
            if content_changed {
                euv_log!("[EUV] content changed, reloading");
                notify_reload(&app_handle);
            } else {
                euv_log!("[EUV] content unchanged, skipping reload");
            }
        }
        Err(error) => euv_log!("[EUV] background update failed: {}", error),
    }
}

/// Derive the base URL for resolving relative resource paths.
/// If the final URL ends with '/', it's already a directory URL — use it as-is (minus trailing slash).
/// Otherwise, strip the last path segment (e.g. "https://x.com/a/b" -> "https://x.com/a").
fn derive_base_url(final_url: &str) -> String {
    let url: String = final_url.trim_end_matches('/').to_string();
    // If the URL ended with '/', the trimmed version is the base.
    if final_url.ends_with('/') {
        return url;
    }
    // Otherwise, strip the last path component
    if let Some(pos) = url.rfind('/') {
        // Make sure we don't strip past the scheme (e.g. "https://")
        if pos > url.find("://").map_or(0, |p| p + 2) {
            return url[..pos].to_string();
        }
    }
    url
}

/// Fetch all linked resources. Returns the total number of successfully fetched resources.
async fn fetch_linked_resources(version_dir: &Path, html: &str, final_url: &str) -> usize {
    let mut paths: Vec<String> = Vec::new();
    extract_attr_values(html, "script", "src", &mut paths);
    extract_attr_values(html, "link", "href", &mut paths);
    extract_attr_values(html, "img", "src", &mut paths);
    extract_module_imports(html, &mut paths);
    let base_url: String = derive_base_url(final_url);
    euv_log!("[EUV] resource base URL: {}", base_url);

    // Phase 1: fetch resources referenced in HTML
    let fetched: Vec<(String, Vec<u8>)> = fetch_resource_list(&paths, &base_url, version_dir).await;
    let mut total_count: usize = fetched.len();

    // Phase 2: scan downloaded JS files for additional dependencies (e.g. .wasm)
    let mut extra_paths: Vec<String> = Vec::new();
    for (clean_path, data) in &fetched {
        if clean_path.ends_with(".js") || clean_path.ends_with(".mjs") {
            let js_content: String = String::from_utf8_lossy(data).to_string();
            extract_wasm_references(&js_content, clean_path, &mut extra_paths);
            extract_module_imports(&js_content, &mut extra_paths);
        }
    }
    // Remove any already-fetched paths
    extra_paths.retain(|p| {
        let clean: String = p.trim_start_matches('/').trim_start_matches("./").to_string();
        !fetched.iter().any(|(c, _)| c == &clean)
    });
    if !extra_paths.is_empty() {
        euv_log!("[EUV] found {} extra dependencies in JS", extra_paths.len());
        let extra_fetched: Vec<(String, Vec<u8>)> =
            fetch_resource_list(&extra_paths, &base_url, version_dir).await;
        total_count += extra_fetched.len();
    }
    total_count
}

async fn fetch_resource_list(
    paths: &[String],
    base_url: &str,
    version_dir: &Path,
) -> Vec<(String, Vec<u8>)> {
    use std::sync::Arc;
    use tokio::sync::Mutex as TokioMutex;

    let results: Arc<TokioMutex<Vec<(String, Vec<u8>)>>> = Arc::new(TokioMutex::new(Vec::new()));
    let mut handles: Vec<tauri::async_runtime::JoinHandle<()>> = Vec::new();

    for relative_path in paths {
        if relative_path.starts_with("data:")
            || relative_path.starts_with("http://")
            || relative_path.starts_with("https://")
            || relative_path.starts_with("//")
        {
            continue;
        }
        let clean: String = relative_path
            .trim_start_matches('/')
            .trim_start_matches("./")
            .to_string();
        let url: String = format!("{}/{}", base_url, clean);
        let local: PathBuf = version_dir.join(&clean);
        let results_clone: Arc<TokioMutex<Vec<(String, Vec<u8>)>>> = results.clone();
        handles.push(tauri::async_runtime::spawn(async move {
            match fetch_url(&url).await {
                Ok(data) => {
                    if data.is_empty() {
                        euv_log!("[EUV] skipped empty resource: {}", clean);
                        return;
                    }
                    if let Err(error) = atomic_write(&local, &data).await {
                        euv_log!("[EUV] write failed {}: {}", clean, error);
                    } else {
                        euv_log!("[EUV] fetched: {} ({} bytes)", clean, data.len());
                        results_clone.lock().await.push((clean, data));
                    }
                }
                Err(error) => {
                    euv_log!("[EUV] fetch failed {}: {}", clean, error);
                }
            }
        }));
    }
    for handle in handles {
        handle.await.ok();
    }
    let mutex = Arc::try_unwrap(results).unwrap_or_else(|arc| {
        // If there are still other references (shouldn't happen since we joined all),
        // create a new mutex with the cloned data
        TokioMutex::new(arc.blocking_lock().clone())
    });
    mutex.into_inner()
}

/// Extract .wasm file references from JS content.
/// Handles patterns like: new URL('euv_bg.wasm', import.meta.url)
/// and: fetch(new URL('file.wasm', ...))
fn extract_wasm_references(js: &str, js_path: &str, results: &mut Vec<String>) {
    // Determine the directory of the JS file for resolving relative paths
    let js_dir: &str = if let Some(pos) = js_path.rfind('/') {
        &js_path[..pos]
    } else {
        ""
    };

    let mut pos: usize = 0;
    while pos < js.len() {
        // Look for quoted strings ending in .wasm
        let index: usize = match js[pos..].find(".wasm") {
            Some(offset) => pos + offset,
            None => break,
        };
        // Find the start of the quoted string containing this .wasm reference
        let search_start: usize = if index > 60 { index - 60 } else { 0 };
        let before: &str = &js[search_start..index];
        let wasm_ref: Option<&str> = before
            .rfind('\'')
            .map(|q| &js[search_start + q + 1..index + 5])
            .or_else(|| {
                before
                    .rfind('"')
                    .map(|q| &js[search_start + q + 1..index + 5])
            });
        if let Some(wasm_file) = wasm_ref {
            // Only accept simple relative filenames (no absolute URLs)
            if !wasm_file.contains("://") && !wasm_file.contains(' ') {
                let resolved: String = if js_dir.is_empty() {
                    wasm_file.to_string()
                } else {
                    format!("{}/{}", js_dir, wasm_file)
                };
                if !results.contains(&resolved) {
                    results.push(resolved);
                }
            }
        }
        pos = index + 5;
    }
}

fn extract_module_imports(html: &str, results: &mut Vec<String>) {
    let mut pos: usize = 0;
    while pos < html.len() {
        let index: usize = match html[pos..].find("from") {
            Some(offset) => pos + offset,
            None => break,
        };
        let after: &str = html[index + 4..].trim_start();
        let value: &str = extract_quoted_value(after);
        if !value.is_empty()
            && (value.starts_with("./") || value.starts_with("../"))
            && !results.contains(&value.to_string())
        {
            results.push(value.to_string());
        }
        pos = index + 4;
    }
}

fn extract_attr_values(html: &str, tag: &str, attr: &str, results: &mut Vec<String>) {
    let tag_pattern: String = format!("<{}", tag.to_lowercase());
    let attr_pattern: String = format!("{}=", attr.to_lowercase());
    let mut pos: usize = 0;
    while pos < html.len() {
        let start: usize = match html[pos..].find(&tag_pattern) {
            Some(offset) => pos + offset,
            None => break,
        };
        let end: usize = match html[start..].find('>') {
            Some(offset) => start + offset,
            None => break,
        };
        let content: String = html[start..end].to_lowercase();
        if let Some(attr_position) = content.find(&attr_pattern) {
            let value: &str = extract_quoted_value(&content[attr_position + attr_pattern.len()..]);
            if !value.is_empty() && !results.contains(&value.to_string()) {
                results.push(value.to_string());
            }
        }
        pos = end + 1;
    }
}

fn extract_quoted_value(input: &str) -> &str {
    let trimmed: &str = input.trim_start();
    if let Some(remainder) = trimmed.strip_prefix('"')
        && let Some(end_position) = remainder.find('"')
    {
        return &remainder[..end_position];
    }
    if let Some(remainder) = trimmed.strip_prefix('\'')
        && let Some(end_position) = remainder.find('\'')
    {
        return &remainder[..end_position];
    }
    ""
}

pub(crate) fn notify_reload(app_handle: &AppHandle) {
    use tauri::Emitter;
    if let Err(error) = app_handle.emit("euv://reload", ()) {
        log::warn!("[EUV] failed to emit reload event: {}", error);
    } else {
        log::info!("[EUV] reload event emitted");
    }
}

pub(crate) fn mime_for(path: &str) -> &'static str {
    let lower_path: String = path.to_lowercase();
    if lower_path.ends_with(".html") || lower_path.ends_with(".htm") {
        "text/html"
    } else if lower_path.ends_with(".js") || lower_path.ends_with(".mjs") {
        "application/javascript"
    } else if lower_path.ends_with(".wasm") {
        "application/wasm"
    } else if lower_path.ends_with(".css") {
        "text/css"
    } else if lower_path.ends_with(".json") {
        "application/json"
    } else if lower_path.ends_with(".png") {
        "image/png"
    } else if lower_path.ends_with(".jpg") || lower_path.ends_with(".jpeg") {
        "image/jpeg"
    } else if lower_path.ends_with(".gif") {
        "image/gif"
    } else if lower_path.ends_with(".svg") {
        "image/svg+xml"
    } else if lower_path.ends_with(".ico") {
        "image/x-icon"
    } else if lower_path.ends_with(".woff") {
        "font/woff"
    } else if lower_path.ends_with(".woff2") {
        "font/woff2"
    } else if lower_path.ends_with(".ttf") {
        "font/ttf"
    } else if lower_path.ends_with(".otf") {
        "font/otf"
    } else if lower_path.ends_with(".webp") {
        "image/webp"
    } else {
        "application/octet-stream"
    }
}

const RELOAD_LISTENER_SCRIPT: &str = r#"<script>
(function(){
  if(window.__TAURI__&&window.__TAURI__.event){
    window.__TAURI__.event.listen('euv://reload',function(){
      window.location.reload();
    });
  } else {
    document.addEventListener('DOMContentLoaded',function(){
      var t=setInterval(function(){
        if(window.__TAURI__&&window.__TAURI__.event){
          clearInterval(t);
          window.__TAURI__.event.listen('euv://reload',function(){
            window.location.reload();
          });
        }
      },100);
      setTimeout(function(){clearInterval(t);},10000);
    });
  }
})();
</script>"#;

#[cfg(debug_assertions)]
const DEBUG_PANEL_SCRIPT: &str = r#"<script>
(function(){
  var expanded=false;
  var bar=document.createElement('div');
  bar.id='__euv_debug_bar';
  bar.style.cssText='position:fixed;bottom:0;left:0;right:0;height:36px;background:#1a1a2e;color:#0f0;font:13px/36px monospace;padding:0 12px;z-index:2147483647;cursor:pointer;user-select:none;display:flex;align-items:center;box-shadow:0 -2px 8px rgba(0,0,0,0.5);';
  bar.textContent='\u25B2 [DEBUG] source: {{SOURCE}} | tap to expand';

  var panel=document.createElement('div');
  panel.id='__euv_debug_panel';
  panel.style.cssText='position:fixed;bottom:0;left:0;right:0;height:50vh;background:#0d0d1a;color:#0f0;font:12px monospace;z-index:2147483646;display:none;flex-direction:column;box-shadow:0 -4px 16px rgba(0,0,0,0.7);';

  var header=document.createElement('div');
  header.style.cssText='padding:8px 12px;background:#1a1a2e;border-bottom:1px solid #333;flex-shrink:0;display:flex;justify-content:space-between;align-items:center;';
  header.innerHTML='<span style="color:#0f0;font-weight:bold;">EUV Debug Console</span><span id="__euv_close" style="color:#f55;cursor:pointer;font-size:18px;">\u2716</span>';

  var info=document.createElement('div');
  info.style.cssText='padding:6px 12px;background:#111;border-bottom:1px solid #222;flex-shrink:0;color:#aaa;font-size:11px;word-break:break-all;';
  info.textContent='source: {{SOURCE}} | path: {{PATH}}';

  var logArea=document.createElement('div');
  logArea.id='__euv_debug_logs';
  logArea.style.cssText='flex:1;overflow-y:auto;padding:8px 12px;';

  panel.appendChild(header);
  panel.appendChild(info);
  panel.appendChild(logArea);

  function toggle(){
    expanded=!expanded;
    if(expanded){
      panel.style.display='flex';
      bar.style.bottom='50vh';
      bar.textContent='\u25BC [DEBUG] source: {{SOURCE}} | tap to collapse';
    } else {
      panel.style.display='none';
      bar.style.bottom='0';
      bar.textContent='\u25B2 [DEBUG] source: {{SOURCE}} | tap to expand';
    }
  }
  bar.addEventListener('click',toggle);

  function addLog(msg){
    var line=document.createElement('div');
    line.style.cssText='padding:2px 0;border-bottom:1px solid #1a1a2e;color:#0f0;word-break:break-all;';
    var now=new Date();
    var ts=now.getHours().toString().padStart(2,'0')+':'+now.getMinutes().toString().padStart(2,'0')+':'+now.getSeconds().toString().padStart(2,'0')+'.'+now.getMilliseconds().toString().padStart(3,'0');
    line.textContent='['+ts+'] '+msg;
    logArea.appendChild(line);
    logArea.scrollTop=logArea.scrollHeight;
  }

  function initListener(){
    if(window.__TAURI__&&window.__TAURI__.event){
      window.__TAURI__.event.listen('euv://debug-log',function(e){
        addLog(e.payload||'');
      });
      addLog('[panel] listener registered');
      addLog('[panel] source: {{SOURCE}}');
      addLog('[panel] path: {{PATH}}');
    } else {
      var t=setInterval(function(){
        if(window.__TAURI__&&window.__TAURI__.event){
          clearInterval(t);
          window.__TAURI__.event.listen('euv://debug-log',function(e){
            addLog(e.payload||'');
          });
          addLog('[panel] listener registered');
          addLog('[panel] source: {{SOURCE}}');
          addLog('[panel] path: {{PATH}}');
        }
      },100);
      setTimeout(function(){clearInterval(t);},10000);
    }
  }

  document.addEventListener('DOMContentLoaded',function(){
    document.body.appendChild(panel);
    document.body.appendChild(bar);
    document.getElementById('__euv_close').addEventListener('click',function(e){
      e.stopPropagation();
      toggle();
    });
    initListener();
  });
})();
</script>"#;

pub(crate) fn handle_euv_scheme(
    app_handle: &AppHandle,
    request: http::Request<Vec<u8>>,
) -> http::Response<Cow<'static, [u8]>> {
    let path: &str = request.uri().path();
    let path_trimmed: &str = path.trim_start_matches('/');
    let path_decoded: String = percent_encoding::percent_decode_str(path_trimmed)
        .decode_utf8_lossy()
        .into_owned();
    let cache_root: PathBuf = match get_cache_root(app_handle) {
        Ok(directory) => directory,
        Err(_) => {
            return http::Response::builder()
                .status(500)
                .body(Cow::Owned(b"Internal error".to_vec()))
                .unwrap();
        }
    };
    let version_name: String = {
        let mut version: Option<String> = get_serving_version();
        if version.is_none() {
            let start: Instant = Instant::now();
            let timeout: Duration = Duration::from_secs(2);
            while version.is_none() && start.elapsed() < timeout {
                std::thread::sleep(Duration::from_millis(2));
                version = get_serving_version();
            }
        }
        match version {
            Some(resolved_version) => resolved_version,
            None => {
                return http::Response::builder()
                    .status(503)
                    .body(Cow::Owned(b"Cache not ready".to_vec()))
                    .unwrap();
            }
        }
    };
    let active_dir: PathBuf = cache_root.join(&version_name);
    let is_index: bool = path_decoded.is_empty() || path_decoded == "index.html";
    let file_path: PathBuf = if is_index {
        active_dir.join("index.html")
    } else {
        active_dir.join(&path_decoded)
    };
    #[cfg(debug_assertions)]
    {
        let serve_path: String = file_path.to_string_lossy().into_owned();
        euv_log!("[EUV] serving: {}", serve_path);
    }
    match std::fs::read(&file_path) {
        Ok(data) => {
            let mime: &str = mime_for(&path_decoded);
            let mut builder: http::response::Builder = http::Response::builder()
                .status(200)
                .header("Content-Type", mime)
                .header("Cache-Control", "no-store, no-cache, must-revalidate")
                .header("Pragma", "no-cache")
                .header("Access-Control-Allow-Origin", "*");
            if path_decoded.ends_with(".wasm") {
                builder = builder
                    .header("Cross-Origin-Embedder-Policy", "require-corp")
                    .header("Cross-Origin-Opener-Policy", "same-origin");
            }
            let body: Vec<u8> = if is_index {
                let html: String = String::from_utf8_lossy(&data).into_owned();
                #[allow(unused_mut)]
                let mut extra_scripts: String = RELOAD_LISTENER_SCRIPT.to_string();
                #[cfg(debug_assertions)]
                {
                    let source: String = get_serving_source();
                    let dir_path: String = active_dir.to_string_lossy().into_owned();
                    extra_scripts.push_str(
                        &DEBUG_PANEL_SCRIPT
                            .replace("{{SOURCE}}", &source)
                            .replace("{{PATH}}", &dir_path)
                            .to_string(),
                    );
                }
                let injected: String = if let Some(pos) = html.find("</head>") {
                    format!("{}{}{}", &html[..pos], extra_scripts, &html[pos..])
                } else if let Some(pos) = html.find("<body") {
                    format!("{}{}{}", &html[..pos], extra_scripts, &html[pos..])
                } else {
                    format!("{}{}", extra_scripts, html)
                };
                injected.into_bytes()
            } else {
                data
            };
            builder.body(Cow::Owned(body)).unwrap()
        }
        Err(_) => http::Response::builder()
            .status(404)
            .header("Content-Type", "text/plain")
            .body(Cow::Owned(b"Not found".to_vec()))
            .unwrap(),
    }
}

#[tauri::command]
pub(crate) async fn load_cached_resource(app_handle: AppHandle) -> Result<CachedPage, String> {
    let from_cache: bool = load_cached_html(&app_handle).await.is_some();
    let source: String = get_serving_source();
    let cache_path: String = get_serving_version()
        .and_then(|version: String| {
            get_cache_root(&app_handle)
                .ok()
                .map(|root: PathBuf| root.join(version).to_string_lossy().into_owned())
        })
        .unwrap_or_default();
    Ok(CachedPage {
        from_cache,
        remote_url: REMOTE_URL.to_string(),
        source,
        cache_path,
    })
}

pub(crate) fn ensure_serving_version_sync(app_handle: &AppHandle) {
    if get_serving_version().is_some() {
        return;
    }
    let cache_root: PathBuf = match get_cache_root(app_handle) {
        Ok(directory) => directory,
        Err(_) => return,
    };
    if let Some(existing) = read_active_version_sync(&cache_root) {
        set_serving_version(&existing);
        set_serving_source("fetched");
        euv_log!("[EUV] reusing existing active deployment: {existing}");
        return;
    }
    if BUNDLED_FILES.is_empty() {
        return;
    }
    std::fs::create_dir_all(&cache_root).ok();
    let version_name: String = format!(
        "{}{}",
        VERSION_PREFIX,
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
    );
    let version_dir: PathBuf = cache_root.join(&version_name);
    if std::fs::create_dir_all(&version_dir).is_err() {
        return;
    }
    let mut count: usize = 0;
    for (rel_path, data) in BUNDLED_FILES {
        let target: PathBuf = version_dir.join(rel_path);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        if std::fs::write(&target, data).is_ok() {
            count += 1;
        }
    }
    if count == 0 {
        return;
    }
    let pointer: PathBuf = cache_root.join(ACTIVE_LINK);
    let temporary: PathBuf = cache_root.join(format!(".active_tmp_{}", std::process::id()));
    if std::fs::write(&temporary, &version_name).is_ok() {
        std::fs::rename(&temporary, &pointer).ok();
    }
    set_serving_version(&version_name);
    set_serving_source("bundled");
    euv_log!("[EUV] deployed {count} bundled files sync, serving: {version_name}");
}
