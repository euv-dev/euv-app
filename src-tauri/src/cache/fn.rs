use crate::*;

pub(crate) fn get_cache_root(app_handle: &AppHandle) -> Result<PathBuf, CacheError> {
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

fn switch_active_version_sync(cache_root: &Path, version_name: &str) -> Result<(), CacheError> {
    let pointer: PathBuf = active_pointer_path(cache_root);
    let temporary: PathBuf = cache_root.join(format!(".active_tmp_{}", std::process::id()));
    std::fs::write(&temporary, version_name)
        .map_err(|error: std::io::Error| CacheError::Write(error.to_string()))?;
    std::fs::rename(&temporary, &pointer)
        .map_err(|error: std::io::Error| CacheError::Write(error.to_string()))?;
    euv_log!("[EUV] switched active: {}", version_name);
    Ok(())
}

async fn cleanup_old_versions(cache_root: &Path, current: &str) {
    let mut read_dir_result: tokio::fs::ReadDir = match read_dir(cache_root).await {
        Ok(entries) => entries,
        Err(_) => return,
    };
    let mut versions: Vec<String> = Vec::new();
    while let Ok(Some(entry)) = read_dir_result.next_entry().await {
        let name: String = entry.file_name().to_string_lossy().to_string();
        if name.starts_with(VERSION_PREFIX)
            && name != current
            && let Ok(metadata) = entry.metadata().await
            && metadata.is_dir()
        {
            versions.push(name);
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

pub(crate) fn deploy_bundled_cache_sync(app_handle: &AppHandle) -> bool {
    if BUNDLED_FILES.is_empty() {
        return false;
    }
    let cache_root: PathBuf = match get_cache_root(app_handle) {
        Ok(directory) => directory,
        Err(_) => return false,
    };
    if let Some(existing) = read_active_version_sync(&cache_root) {
        euv_log!("[EUV] reusing existing active deployment: {existing}");
        return true;
    }
    if std::fs::create_dir_all(&cache_root).is_err() {
        return false;
    }
    let version_name: String = new_version_name();
    let version_dir: PathBuf = cache_root.join(&version_name);
    if std::fs::create_dir_all(&version_dir).is_err() {
        return false;
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
        std::fs::remove_dir_all(&version_dir).ok();
        return false;
    }
    if switch_active_version_sync(&cache_root, &version_name).is_err() {
        return false;
    }
    euv_log!("[EUV] deployed {count} bundled files, serving: {version_name}");
    true
}

pub(crate) fn has_active_cache(app_handle: &AppHandle) -> bool {
    let cache_root: PathBuf = match get_cache_root(app_handle) {
        Ok(directory) => directory,
        Err(_) => return false,
    };
    read_active_version_sync(&cache_root).is_some()
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
    let mut expected_paths: Vec<String> = Vec::new();
    extract_attr_values(&html, "script", "src", &mut expected_paths);
    extract_attr_values(&html, "link", "href", &mut expected_paths);
    extract_attr_values(&html, "img", "src", &mut expected_paths);
    extract_module_imports(&html, &mut expected_paths);
    expected_paths.retain(|p| {
        !p.starts_with("http://")
            && !p.starts_with("https://")
            && !p.starts_with("//")
            && !p.starts_with("data:")
    });
    if resource_count == 0 && !expected_paths.is_empty() {
        remove_dir_all(&version_dir).await.ok();
        return Err(CacheError::Fetch(
            "incomplete snapshot: no resources fetched".to_string(),
        ));
    }
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
    let mut wasm_paths: Vec<String> = Vec::new();
    let mut js_to_scan: Vec<String> = expected_paths
        .iter()
        .map(|p| {
            p.trim_start_matches('/')
                .trim_start_matches("./")
                .to_string()
        })
        .filter(|c| c.ends_with(".js") || c.ends_with(".mjs"))
        .collect();
    let pkg_dir: PathBuf = version_dir.join("pkg");
    if pkg_dir.exists()
        && let Ok(mut entries) = tokio::fs::read_dir(&pkg_dir).await
    {
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

pub(crate) async fn initial_fetch(app_handle: AppHandle) {
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

pub(crate) async fn background_update(app_handle: AppHandle) {
    euv_log!("[EUV] background update started");
    let cache_root: PathBuf = match get_cache_root(&app_handle) {
        Ok(directory) => directory,
        Err(_) => return,
    };
    create_dir_all(&cache_root).await.ok();
    loop {
        match fetch_full_snapshot(&cache_root).await {
            Ok(version) => {
                euv_log!(
                    "[EUV] background update done: {}, will take effect on next launch",
                    version
                );
                return;
            }
            Err(error) => {
                euv_log!("[EUV] background update failed: {}, retrying", error);
                tokio::time::sleep(Duration::from_millis(RETRY_INTERVAL_MILLIS)).await;
            }
        }
    }
}

fn derive_base_url(final_url: &str) -> String {
    let url: String = final_url.trim_end_matches('/').to_string();
    if final_url.ends_with('/') {
        return url;
    }
    if let Some(pos) = url.rfind('/')
        && pos > url.find("://").map_or(0, |p| p + 2)
    {
        return url[..pos].to_string();
    }
    url
}

async fn fetch_linked_resources(version_dir: &Path, html: &str, final_url: &str) -> usize {
    let mut paths: Vec<String> = Vec::new();
    extract_attr_values(html, "script", "src", &mut paths);
    extract_attr_values(html, "link", "href", &mut paths);
    extract_attr_values(html, "img", "src", &mut paths);
    extract_module_imports(html, &mut paths);
    let base_url: String = derive_base_url(final_url);
    euv_log!("[EUV] resource base URL: {}", base_url);
    let fetched: Vec<(String, Vec<u8>)> = fetch_resource_list(&paths, &base_url, version_dir).await;
    let mut total_count: usize = fetched.len();
    let mut extra_paths: Vec<String> = Vec::new();
    for (clean_path, data) in &fetched {
        if clean_path.ends_with(".js") || clean_path.ends_with(".mjs") {
            let js_content: String = String::from_utf8_lossy(data).to_string();
            extract_wasm_references(&js_content, clean_path, &mut extra_paths);
            extract_module_imports(&js_content, &mut extra_paths);
        }
    }
    extra_paths.retain(|p| {
        let clean: String = p
            .trim_start_matches('/')
            .trim_start_matches("./")
            .to_string();
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

async fn fetch_resource_list(paths: &[String], base_url: &str, version_dir: &Path) -> FetchResult {
    let results: SharedResults = Arc::new(tokio::sync::Mutex::new(Vec::new()));
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
        let results_clone: SharedResults = results.clone();
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
    let mutex = Arc::try_unwrap(results)
        .unwrap_or_else(|arc: SharedResults| tokio::sync::Mutex::new(arc.blocking_lock().clone()));
    mutex.into_inner()
}

fn extract_wasm_references(js: &str, js_path: &str, results: &mut Vec<String>) {
    let js_dir: &str = if let Some(pos) = js_path.rfind('/') {
        &js_path[..pos]
    } else {
        ""
    };
    let mut pos: usize = 0;
    while pos < js.len() {
        let index: usize = match js[pos..].find(".wasm") {
            Some(offset) => pos + offset,
            None => break,
        };
        let search_start: usize = index.saturating_sub(60);
        let before: &str = &js[search_start..index];
        let wasm_ref: Option<&str> = before
            .rfind('\'')
            .map(|q| &js[search_start + q + 1..index + 5])
            .or_else(|| {
                before
                    .rfind('"')
                    .map(|q| &js[search_start + q + 1..index + 5])
            });
        if let Some(wasm_file) = wasm_ref
            && !wasm_file.contains("://")
            && !wasm_file.contains(' ')
        {
            let resolved: String = if js_dir.is_empty() {
                wasm_file.to_string()
            } else {
                format!("{}/{}", js_dir, wasm_file)
            };
            if !results.contains(&resolved) {
                results.push(resolved);
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
    let active_version: Option<String> = read_active_version_sync(&cache_root);
    let active_dir: PathBuf = match active_version.as_deref() {
        Some(version) => cache_root.join(version),
        None => {
            let start: Instant = Instant::now();
            let timeout: Duration = Duration::from_secs(2);
            loop {
                if let Some(version) = read_active_version_sync(&cache_root) {
                    break cache_root.join(version);
                }
                if start.elapsed() >= timeout {
                    return http::Response::builder()
                        .status(503)
                        .body(Cow::Owned(b"Cache not ready".to_vec()))
                        .unwrap();
                }
                std::thread::sleep(Duration::from_millis(2));
            }
        }
    };
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
                    let source: String = read_active_version_sync(&cache_root)
                        .map(|_| "active".to_string())
                        .unwrap_or_else(|| "none".to_string());
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
    let cache_root: PathBuf =
        get_cache_root(&app_handle).map_err(|error: CacheError| error.to_string())?;
    let active_version: Option<String> = read_active_version_sync(&cache_root);
    let from_cache: bool = active_version.is_some();
    let source: String = active_version
        .as_ref()
        .map(|_| "active".to_string())
        .unwrap_or_else(|| "none".to_string());
    let cache_path: String = active_version
        .as_deref()
        .map(|version: &str| cache_root.join(version).to_string_lossy().into_owned())
        .unwrap_or_default();
    Ok(CachedPage {
        from_cache,
        remote_url: REMOTE_URL.to_string(),
        source,
        cache_path,
    })
}
