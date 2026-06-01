use crate::cache::*;
use crate::cache::r#fn::*;
use crate::cache::r#struct::*;
use crate::cache::r#const::*;

use std::path::PathBuf;
use tauri::Manager;

/// Gets the cache directory path.
pub fn get_cache_dir(app_handle: &tauri::AppHandle) -> Result<PathBuf, CacheError> {
    let mut dir = app_handle
        .path()
        .app_cache_dir()
        .map_err(|e| CacheError::Read(format!("{}", e)))?;
    dir.push(CACHE_DIR);
    Ok(dir)
}

/// Writes a debug marker file to prove the command was called.
fn write_debug_marker(app_handle: &tauri::AppHandle, msg: &str) {
    if let Ok(mut dir) = app_handle.path().app_cache_dir() {
        dir.push("debug.log");
        let _ = std::fs::write(&dir, msg);
    }
}

/// Fetches a remote URL and returns (content_bytes, content_type).
pub async fn fetch_url(url: &str) -> Result<(Vec<u8>, String), CacheError> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(FETCH_TIMEOUT_SECS))
        .build()
        .map_err(|e| CacheError::Fetch(e.to_string()))?;

    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| CacheError::Fetch(e.to_string()))?;

    let status = resp.status();
    if !status.is_success() {
        return Err(CacheError::Fetch(format!("HTTP {}", status)));
    }

    let content_type = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/octet-stream")
        .to_string();

    let bytes = resp
        .bytes()
        .await
        .map_err(|e| CacheError::Fetch(e.to_string()))?;

    if bytes.len() > MAX_BODY_SIZE {
        return Err(CacheError::Fetch(format!("Too large: {} bytes", bytes.len())));
    }

    Ok((bytes.to_vec(), content_type))
}

/// Fetches and caches a single resource.
async fn fetch_and_cache_resource(url: &str, cache_dir: &std::path::Path) -> Result<(), CacheError> {
    if let Some(local_path) = url_to_cache_path(cache_dir, url) {
        if local_path.exists() {
            return Ok(());
        }
        if let Ok((bytes, _ct)) = fetch_url(url).await {
            if let Some(parent) = local_path.parent() {
                std::fs::create_dir_all(parent).map_err(|e| CacheError::Write(e.to_string()))?;
            }
            std::fs::write(&local_path, &bytes).map_err(|e| CacheError::Write(e.to_string()))?;
        }
    }
    Ok(())
}

/// Main function: loads the page, fetches all resources, returns rewritten HTML.
pub async fn load_page(app_handle: &tauri::AppHandle) -> Result<CachedPage, CacheError> {
    println!("[EUV] load_page() started");
    let cache_dir = get_cache_dir(app_handle)?;
    println!("[EUV] cache_dir: {:?}", cache_dir);
    std::fs::create_dir_all(&cache_dir).ok();

    // Fetch fresh HTML from network
    println!("[EUV] fetching: {}", REMOTE_URL);
    let (html_bytes, ct) = fetch_url(REMOTE_URL).await?;
    println!("[EUV] fetched {} bytes, content-type: {}", html_bytes.len(), ct);
    let html = String::from_utf8_lossy(&html_bytes).to_string();

    // Extract all resource URLs
    let resources = extract_resource_urls(&html, REMOTE_URL);
    println!("[EUV] found {} resources", resources.len());

    // Fetch and cache all resources concurrently
    let mut tasks = Vec::new();
    for (url, _kind) in &resources {
        tasks.push(fetch_and_cache_resource(url, &cache_dir));
    }
    for task in tasks {
        let _ = task.await;
    }

    // Rewrite HTML URLs to point to local files
    let rewritten_html = rewrite_html_urls(&html, REMOTE_URL, &cache_dir);

    // Save the rewritten HTML
    let index_path = cache_dir.join("ltpp.vip").join("euv").join("index.html");
    if let Some(parent) = index_path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    std::fs::write(&index_path, &rewritten_html)
        .map_err(|e| CacheError::Write(e.to_string()))?;

    let has_cache = true; // We just wrote it

    println!("[EUV] load_page() done, {} resources cached", resources.len());

    Ok(CachedPage {
        html: rewritten_html,
        from_cache: has_cache,
        resource_count: resources.len(),
    })
}

/// Tauri command: load the cached/fresh page.
#[tauri::command]
pub async fn load_cached_resource(
    app_handle: tauri::AppHandle,
) -> Result<CachedPage, String> {
    println!("[EUV] === load_cached_resource command called! ===");
    write_debug_marker(&app_handle, &format!("command_called_{}", std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()));

    load_page(&app_handle).await.map_err(|e| {
        let _ = write_debug_marker(&app_handle, &format!("error_{}", e));
        e.to_string()
    })
}

/// Background task: update all cached resources.
pub async fn update_cache_async(app_handle: tauri::AppHandle) {
    println!("[EUV] background cache update started");
    write_debug_marker(&app_handle, "background_update_started");
    match load_page(&app_handle).await {
        Ok(page) => {
            println!("[EUV] background cache done, {} resources", page.resource_count);
            write_debug_marker(&app_handle, &format!("background_done_{}", page.resource_count));
        }
        Err(e) => {
            println!("[EUV] background cache error: {}", e);
            write_debug_marker(&app_handle, &format!("background_error_{}", e));
        }
    }
}
