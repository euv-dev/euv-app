use crate::*;

/// Gets the cache directory path.
pub(crate) fn get_cache_dir(app_handle: &tauri::AppHandle) -> Result<PathBuf, CacheError> {
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
pub(crate) async fn fetch_url(url: &str) -> Result<(Vec<u8>, String), CacheError> {
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
        return Err(CacheError::Fetch(format!(
            "Too large: {} bytes",
            bytes.len()
        )));
    }

    Ok((bytes.to_vec(), content_type))
}

/// Main function: loads the page HTML (from cache or network).
/// Injects a <base href> tag so that relative resource URLs resolve correctly
/// when the HTML is rendered via document.write() in the WebView.
pub(crate) async fn load_page(app_handle: &tauri::AppHandle) -> Result<CachedPage, CacheError> {
    println!("[EUV] load_page() started");
    let cache_dir = get_cache_dir(app_handle)?;
    println!("[EUV] cache_dir: {:?}", cache_dir);
    std::fs::create_dir_all(&cache_dir).ok();

    // Try to load cached HTML first
    let index_path = cache_dir.join("index.html");
    let (html, from_cache) = if index_path.exists() {
        match std::fs::read_to_string(&index_path) {
            Ok(cached_html) if !cached_html.is_empty() => {
                println!("[EUV] loaded HTML from cache");
                (cached_html, true)
            }
            _ => {
                // Cache file corrupt, fetch fresh
                let fresh = fetch_fresh_html(&index_path).await?;
                (fresh, false)
            }
        }
    } else {
        // No cache, fetch from network
        let fresh = fetch_fresh_html(&index_path).await?;
        (fresh, false)
    };

    // Inject <base href> so relative URLs resolve to the remote server.
    // This is critical for document.write() rendering on iOS.
    let html_with_base = inject_base_href(&html);

    println!("[EUV] load_page() done, from_cache={}", from_cache);

    Ok(CachedPage {
        html: html_with_base,
        from_cache,
        resource_count: 0,
    })
}

/// Injects <base href> and viewport-fit=cover into the HTML <head>.
/// - <base href> ensures relative resource URLs resolve to the remote server.
/// - viewport-fit=cover ensures content extends into iOS safe areas.
fn inject_base_href(html: &str) -> String {
    let base_tag = format!("<base href=\"{}\">", REMOTE_URL);
    let viewport_meta = "<meta name=\"viewport\" content=\"width=device-width, initial-scale=1.0, maximum-scale=1.0, user-scalable=no, viewport-fit=cover\">";

    let mut result = html.to_string();

    // Inject <base href> after <head>
    if let Some(pos) = result.find("<head>") {
        let insert_pos = pos + "<head>".len();
        result.insert_str(insert_pos, &base_tag);
    } else if let Some(pos) = result.find("<head ") {
        if let Some(end) = result[pos..].find('>') {
            let insert_pos = pos + end + 1;
            result.insert_str(insert_pos, &base_tag);
        } else {
            result = format!("{}{}", base_tag, result);
        }
    } else {
        result = format!("{}{}", base_tag, result);
    }

    // Ensure viewport-fit=cover is present
    if !result.contains("viewport-fit") {
        // Replace existing viewport meta or inject new one
        if let Some(vp_start) = result.find("<meta name=\"viewport\"") {
            if let Some(vp_end) = result[vp_start..].find('>') {
                let end_pos: usize = vp_start + vp_end + 1;
                result = format!(
                    "{}{}{}",
                    &result[..vp_start],
                    viewport_meta,
                    &result[end_pos..]
                );
            }
        } else {
            // No viewport meta found, inject after <base>
            if let Some(base_pos) = result.find(&base_tag) {
                let insert_pos = base_pos + base_tag.len();
                result.insert_str(insert_pos, viewport_meta);
            }
        }
    }

    result
}

/// Fetches fresh HTML from the remote URL and saves it to cache.
async fn fetch_fresh_html(index_path: &std::path::Path) -> Result<String, CacheError> {
    println!("[EUV] fetching: {}", REMOTE_URL);
    let (html_bytes, ct) = fetch_url(REMOTE_URL).await?;
    println!(
        "[EUV] fetched {} bytes, content-type: {}",
        html_bytes.len(),
        ct
    );
    let html = String::from_utf8_lossy(&html_bytes).to_string();

    // Save original HTML to cache for next launch
    if let Some(parent) = index_path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    std::fs::write(index_path, &html).map_err(|e| CacheError::Write(e.to_string()))?;
    println!("[EUV] saved HTML to cache");

    Ok(html)
}

/// Tauri command: load the cached/fresh page.
#[tauri::command]
pub(crate) async fn load_cached_resource(
    app_handle: tauri::AppHandle,
) -> Result<CachedPage, String> {
    println!("[EUV] === load_cached_resource command called! ===");
    write_debug_marker(
        &app_handle,
        &format!(
            "command_called_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs()
        ),
    );

    load_page(&app_handle).await.map_err(|e| {
        write_debug_marker(&app_handle, &format!("error_{}", e));
        e.to_string()
    })
}

/// Background task: pre-fetch and cache the remote HTML for next launch.
pub(crate) async fn update_cache_async(app_handle: tauri::AppHandle) {
    println!("[EUV] background cache update started");
    write_debug_marker(&app_handle, "background_update_started");
    match get_cache_dir(&app_handle) {
        Ok(cache_dir) => {
            std::fs::create_dir_all(&cache_dir).ok();
            let index_path = cache_dir.join("index.html");
            match fetch_fresh_html(&index_path).await {
                Ok(_) => {
                    println!("[EUV] background cache update done");
                    write_debug_marker(&app_handle, "background_done");
                }
                Err(e) => {
                    println!("[EUV] background cache error: {}", e);
                    write_debug_marker(&app_handle, &format!("background_error_{}", e));
                }
            }
        }
        Err(e) => {
            println!("[EUV] background cache dir error: {}", e);
        }
    }
}
