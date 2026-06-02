use crate::cache::*;
use std::borrow::Cow;
use std::path::PathBuf;

pub(crate) fn get_cache_root(app_handle: &tauri::AppHandle) -> Result<PathBuf, CacheError> {
    use tauri::Manager;
    let mut dir = app_handle.path().app_cache_dir().map_err(|e| CacheError::Read(e.to_string()))?;
    dir.push(CACHE_DIR);
    Ok(dir)
}

fn active_pointer_path(cache_root: &std::path::Path) -> PathBuf {
    cache_root.join(ACTIVE_LINK)
}

async fn read_active_version(cache_root: &std::path::Path) -> Option<String> {
    let pointer = active_pointer_path(cache_root);
    let name = tokio::fs::read_to_string(&pointer).await.ok()?;
    let name = name.trim().to_string();
    if name.is_empty() { return None; }
    let dir = cache_root.join(&name);
    if tokio::fs::metadata(&dir).await.ok()?.is_dir() { Some(name) } else { None }
}

async fn get_active_cache_dir(cache_root: &std::path::Path) -> Option<PathBuf> {
    let name = read_active_version(cache_root).await?;
    Some(cache_root.join(name))
}

fn new_version_name() -> String {
    let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis();
    format!("{}{}", VERSION_PREFIX, ts)
}

async fn switch_active_version(cache_root: &std::path::Path, version_name: &str) -> Result<(), CacheError> {
    let pointer = active_pointer_path(cache_root);
    let tmp = cache_root.join(format!(".active_tmp_{}", std::process::id()));
    tokio::fs::write(&tmp, version_name).await.map_err(|e| CacheError::Write(e.to_string()))?;
    tokio::fs::rename(&tmp, &pointer).await.map_err(|e| CacheError::Write(e.to_string()))?;
    log::info!("[EUV] switched active: {}", version_name);
    Ok(())
}

async fn cleanup_old_versions(cache_root: &std::path::Path, current: &str) {
    let mut rd = match tokio::fs::read_dir(cache_root).await { Ok(r) => r, Err(_) => return };
    let mut versions: Vec<String> = Vec::new();
    while let Ok(Some(entry)) = rd.next_entry().await {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with(VERSION_PREFIX) && name != current {
            if let Ok(m) = entry.metadata().await { if m.is_dir() { versions.push(name); } }
        }
    }
    versions.sort();
    let max_old = MAX_KEPT_VERSIONS.saturating_sub(1);
    if versions.len() <= max_old { return; }
    let to_remove = versions.len() - max_old;
    for name in &versions[..to_remove] {
        tokio::fs::remove_dir_all(cache_root.join(name)).await.ok();
        log::info!("[EUV] removed old version: {}", name);
    }
}

pub(crate) async fn fetch_url(url: &str) -> Result<Vec<u8>, CacheError> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(FETCH_TIMEOUT_SECS))
        .redirect(reqwest::redirect::Policy::limited(MAX_REDIRECTS))
        .build().map_err(|e| CacheError::Fetch(e.to_string()))?;
    let resp = client.get(url).send().await.map_err(|e| CacheError::Fetch(e.to_string()))?;
    if !resp.status().is_success() {
        return Err(CacheError::Fetch(format!("HTTP {}", resp.status())));
    }
    let bytes = resp.bytes().await.map_err(|e| CacheError::Fetch(e.to_string()))?.to_vec();
    if bytes.len() > MAX_BODY_SIZE {
        return Err(CacheError::Fetch(format!("Too large: {} bytes", bytes.len())));
    }
    Ok(bytes)
}

async fn atomic_write(target: &std::path::Path, data: &[u8]) -> Result<(), CacheError> {
    if let Some(parent) = target.parent() {
        tokio::fs::create_dir_all(parent).await.map_err(|e| CacheError::Write(e.to_string()))?;
    }
    let tmp = target.with_extension("tmp");
    tokio::fs::write(&tmp, data).await.map_err(|e| CacheError::Write(e.to_string()))?;
    tokio::fs::rename(&tmp, target).await.map_err(|e| CacheError::Write(e.to_string()))?;
    Ok(())
}

pub(crate) async fn load_cached_html(app_handle: &tauri::AppHandle) -> Option<String> {
    let cache_root = get_cache_root(app_handle).ok()?;
    let active_dir = get_active_cache_dir(&cache_root).await?;
    let index_path = active_dir.join("index.html");
    let html = tokio::fs::read_to_string(&index_path).await.ok()?;
    if html.is_empty() { None } else { Some(html) }
}

#[allow(clippy::const_is_empty)]
pub(crate) async fn deploy_bundled_cache(app_handle: &tauri::AppHandle) -> bool {
    let bundled_files = crate::cache::bundled::BUNDLED_FILES;
    if bundled_files.is_empty() { return false; }
    let cache_root = match get_cache_root(app_handle) { Ok(d) => d, Err(_) => return false };
    if tokio::fs::create_dir_all(&cache_root).await.is_err() { return false; }
    let version_name = new_version_name();
    let version_dir = cache_root.join(&version_name);
    if tokio::fs::create_dir_all(&version_dir).await.is_err() { return false; }
    let mut count: usize = 0;
    for (rel_path, data) in bundled_files {
        let target = version_dir.join(rel_path);
        if let Some(parent) = target.parent() { tokio::fs::create_dir_all(parent).await.ok(); }
        if tokio::fs::write(&target, data).await.is_ok() { count += 1; }
    }
    if count == 0 { tokio::fs::remove_dir_all(&version_dir).await.ok(); return false; }
    if switch_active_version(&cache_root, &version_name).await.is_err() { return false; }
    log::info!("[EUV] deployed {} bundled files", count);
    true
}

async fn fetch_full_snapshot(cache_root: &std::path::Path) -> Result<String, CacheError> {
    let html_bytes = fetch_url(REMOTE_URL).await?;
    let html = String::from_utf8_lossy(&html_bytes).to_string();
    if html.is_empty() { return Err(CacheError::Fetch("empty HTML".to_string())); }
    let version_name = new_version_name();
    let version_dir = cache_root.join(&version_name);
    tokio::fs::create_dir_all(&version_dir).await.map_err(|e| CacheError::Write(e.to_string()))?;
    atomic_write(&version_dir.join("index.html"), html.as_bytes()).await?;
    fetch_linked_resources(&version_dir, &html).await;
    switch_active_version(cache_root, &version_name).await?;
    cleanup_old_versions(cache_root, &version_name).await;
    Ok(version_name)
}

pub(crate) async fn initial_fetch_and_notify(app_handle: tauri::AppHandle) {
    log::info!("[EUV] initial fetch started");
    let cache_root = match get_cache_root(&app_handle) { Ok(d) => d, Err(_) => return };
    tokio::fs::create_dir_all(&cache_root).await.ok();
    loop {
        match fetch_full_snapshot(&cache_root).await {
            Ok(v) => { log::info!("[EUV] initial fetch done: {}", v); return; }
            Err(e) => {
                log::warn!("[EUV] initial fetch failed: {}, retrying", e);
                tokio::time::sleep(std::time::Duration::from_millis(RETRY_INTERVAL_MILLIS)).await;
            }
        }
    }
}

pub(crate) async fn update_cache_async(app_handle: tauri::AppHandle) {
    log::info!("[EUV] background update started");
    let cache_root = match get_cache_root(&app_handle) { Ok(d) => d, Err(_) => return };
    tokio::fs::create_dir_all(&cache_root).await.ok();
    match fetch_full_snapshot(&cache_root).await {
        Ok(v) => log::info!("[EUV] background update done: {}", v),
        Err(e) => log::warn!("[EUV] background update failed: {}", e),
    }
}

async fn fetch_linked_resources(version_dir: &std::path::Path, html: &str) {
    let mut paths: Vec<String> = Vec::new();
    extract_attr_values(html, "script", "src", &mut paths);
    extract_attr_values(html, "link", "href", &mut paths);
    extract_attr_values(html, "img", "src", &mut paths);
    extract_module_imports(html, &mut paths);
    let base_url = REMOTE_BASE_URL.trim_end_matches('/');
    for rel in paths {
        if rel.starts_with("data:") || rel.starts_with("http://") || rel.starts_with("https://") || rel.starts_with("//") { continue; }
        let clean = rel.trim_start_matches('/').trim_start_matches("./");
        let url = format!("{}/{}", base_url, clean);
        let local = version_dir.join(clean);
        match fetch_url(&url).await {
            Ok(data) => { if let Err(e) = atomic_write(&local, &data).await { log::warn!("[EUV] write failed {}: {}", clean, e); } }
            Err(e) => { log::warn!("[EUV] fetch failed {}: {}", clean, e); }
        }
    }
}

fn extract_module_imports(html: &str, results: &mut Vec<String>) {
    let mut pos: usize = 0;
    while pos < html.len() {
        let i = match html[pos..].find("from") { Some(p) => pos + p, None => break };
        let after = html[i + 4..].trim_start();
        let val = extract_quoted_value(after);
        if !val.is_empty() && (val.starts_with("./") || val.starts_with("../")) && !results.contains(&val.to_string()) {
            results.push(val.to_string());
        }
        pos = i + 4;
    }
}

fn extract_attr_values(html: &str, tag: &str, attr: &str, results: &mut Vec<String>) {
    let tag_pat = format!("<{}", tag.to_lowercase());
    let attr_pat = format!("{}=", attr.to_lowercase());
    let mut pos: usize = 0;
    while pos < html.len() {
        let start = match html[pos..].find(&tag_pat) { Some(p) => pos + p, None => break };
        let end = match html[start..].find('>') { Some(p) => start + p, None => break };
        let content = html[start..end].to_lowercase();
        if let Some(ap) = content.find(&attr_pat) {
            let val = extract_quoted_value(&content[ap + attr_pat.len()..]);
            if !val.is_empty() && !results.contains(&val.to_string()) { results.push(val.to_string()); }
        }
        pos = end + 1;
    }
}

fn extract_quoted_value(s: &str) -> &str {
    let t = s.trim_start();
    if let Some(r) = t.strip_prefix('"') { if let Some(e) = r.find('"') { return &r[..e]; } }
    if let Some(r) = t.strip_prefix('\'') { if let Some(e) = r.find('\'') { return &r[..e]; } }
    ""
}

pub(crate) fn mime_for(path: &str) -> &'static str {
    let l = path.to_lowercase();
    if l.ends_with(".html") || l.ends_with(".htm") { "text/html" }
    else if l.ends_with(".js") || l.ends_with(".mjs") { "application/javascript" }
    else if l.ends_with(".wasm") { "application/wasm" }
    else if l.ends_with(".css") { "text/css" }
    else if l.ends_with(".json") { "application/json" }
    else if l.ends_with(".png") { "image/png" }
    else if l.ends_with(".jpg") || l.ends_with(".jpeg") { "image/jpeg" }
    else if l.ends_with(".gif") { "image/gif" }
    else if l.ends_with(".svg") { "image/svg+xml" }
    else if l.ends_with(".ico") { "image/x-icon" }
    else if l.ends_with(".woff") { "font/woff" }
    else if l.ends_with(".woff2") { "font/woff2" }
    else if l.ends_with(".ttf") { "font/ttf" }
    else if l.ends_with(".otf") { "font/otf" }
    else if l.ends_with(".webp") { "image/webp" }
    else { "application/octet-stream" }
}

pub(crate) fn handle_euv_scheme(
    app_handle: &tauri::AppHandle,
    request: http::Request<Vec<u8>>,
) -> http::Response<Cow<'static, [u8]>> {
    let path = request.uri().path();
    let path_trimmed = path.trim_start_matches('/');
    let path_decoded = percent_encoding::percent_decode_str(path_trimmed).decode_utf8_lossy().into_owned();
    let cache_root = match get_cache_root(app_handle) {
        Ok(d) => d,
        Err(_) => return http::Response::builder().status(500).body(Cow::Owned(b"Internal error".to_vec())).unwrap(),
    };
    let pointer = active_pointer_path(&cache_root);
    let version_name = match std::fs::read_to_string(&pointer) {
        Ok(n) => n.trim().to_string(),
        Err(_) => return http::Response::builder().status(503).body(Cow::Owned(b"Cache not ready".to_vec())).unwrap(),
    };
    if version_name.is_empty() {
        return http::Response::builder().status(503).body(Cow::Owned(b"Cache not ready".to_vec())).unwrap();
    }
    let active_dir = cache_root.join(&version_name);
    let file_path = if path_decoded.is_empty() || path_decoded == "index.html" {
        active_dir.join("index.html")
    } else {
        active_dir.join(&path_decoded)
    };
    match std::fs::read(&file_path) {
        Ok(data) => {
            let mime = mime_for(&path_decoded);
            let mut b = http::Response::builder()
                .status(200)
                .header("Content-Type", mime)
                .header("Cache-Control", "no-cache")
                .header("Access-Control-Allow-Origin", "*");
            if path_decoded.ends_with(".wasm") {
                b = b.header("Cross-Origin-Embedder-Policy", "require-corp")
                     .header("Cross-Origin-Opener-Policy", "same-origin");
            }
            b.body(Cow::Owned(data)).unwrap()
        }
        Err(_) => http::Response::builder().status(404).header("Content-Type", "text/plain").body(Cow::Owned(b"Not found".to_vec())).unwrap(),
    }
}

#[tauri::command]
pub(crate) async fn load_cached_resource(app_handle: tauri::AppHandle) -> Result<CachedPage, String> {
    let from_cache = load_cached_html(&app_handle).await.is_some();
    Ok(CachedPage { from_cache, remote_url: REMOTE_URL.to_string() })
}
