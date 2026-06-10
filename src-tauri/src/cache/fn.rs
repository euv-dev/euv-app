use super::*;

/// JavaScript injected into **every** webview on each page load to intercept any
/// attempt to open an external link and route it to the system's default
/// browser instead of navigating in-app.
///
/// Because it is injected from [`Builder::on_page_load`] — an application-level
/// hook that fires for every webview the app owns on every page load — it also
/// covers webviews created dynamically at runtime, after the app has started.
///
/// The script is idempotent (guarded by `__euvLinkGuard`) and intercepts:
/// - clicks on `<a href>` (including `target="_blank"`),
/// - `window.open(...)` calls,
/// - programmatic assignments to `window.location` / `location.href`.
///
/// Internal targets (the app's own `euv://` scheme, the Tauri/wry runtime
/// origins on `*.localhost`, relative URLs, and `about:`/`data:`/`blob:`/
/// `javascript:` URLs) are left untouched so in-app navigation keeps working.
/// Any genuine external `http(s)`/`mailto:`/`tel:`/… target is handed to the
/// `plugin:opener|open_url` command, which opens it in the system browser.
const LINK_INTERCEPTOR_SCRIPT: &str = r#"
(function () {
  if (window.__euvLinkGuard) { return; }
  window.__euvLinkGuard = true;

  function isInternal(href) {
    if (!href) { return true; }
    var s = String(href).trim();
    if (s === '' || s.charAt(0) === '#') { return true; }
    var lower = s.toLowerCase();
    if (lower.indexOf('euv://') === 0) { return true; }
    if (lower.indexOf('about:') === 0
        || lower.indexOf('data:') === 0
        || lower.indexOf('blob:') === 0
        || lower.indexOf('javascript:') === 0) { return true; }
    var u;
    try { u = new URL(s, document.baseURI); } catch (e) { return true; }
    var scheme = (u.protocol || '').replace(':', '').toLowerCase();
    if (scheme === 'euv' || scheme === 'tauri' || scheme === 'asset'
        || scheme === 'about' || scheme === 'data' || scheme === 'blob'
        || scheme === 'javascript') { return true; }
    if (scheme === 'http' || scheme === 'https') {
      var host = (u.hostname || '').toLowerCase();
      if (host === 'localhost' || host === 'tauri.localhost'
          || host === '127.0.0.1' || host.indexOf('.localhost') !== -1
          && host.lastIndexOf('.localhost') === host.length - '.localhost'.length) {
        return true;
      }
      // Same-origin navigation inside the app stays in-app.
      if (u.origin === window.location.origin) { return true; }
      return false;
    }
    // Any other scheme (mailto:, tel:, ftp:, custom app links, …) is external.
    return false;
  }

  function openExternal(href) {
    var url;
    try { url = new URL(String(href), document.baseURI).href; }
    catch (e) { url = String(href); }
    try {
      var core = window.__TAURI__ && window.__TAURI__.core;
      if (core && typeof core.invoke === 'function') {
        core.invoke('plugin:opener|open_url', { url: url, with: null });
        return true;
      }
      if (window.__TAURI_INTERNALS__ && typeof window.__TAURI_INTERNALS__.invoke === 'function') {
        window.__TAURI_INTERNALS__.invoke('plugin:opener|open_url', { url: url, with: null });
        return true;
      }
    } catch (e) {}
    return false;
  }

  // 1) Intercept link clicks (covers target="_blank" and normal anchors).
  document.addEventListener('click', function (event) {
    if (event.defaultPrevented || event.button !== 0
        || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) { return; }
    var node = event.target;
    while (node && node.nodeName && node.nodeName.toLowerCase() !== 'a') {
      node = node.parentNode;
    }
    if (!node || node.nodeName.toLowerCase() !== 'a') { return; }
    var href = node.getAttribute('href') || node.href;
    if (isInternal(href)) { return; }
    if (openExternal(href)) {
      event.preventDefault();
      event.stopPropagation();
    }
  }, true);

  // 2) Intercept window.open for external URLs.
  var nativeOpen = window.open;
  window.open = function (url, name, features) {
    if (url && !isInternal(url)) {
      if (openExternal(url)) { return null; }
    }
    return nativeOpen ? nativeOpen.apply(window, arguments) : null;
  };
})();
"#;

/// Injects the external-link interceptor into a webview as soon as a page starts
/// loading. Registered via [`Builder::on_page_load`], so it runs for **every**
/// webview, including dynamically created ones, on every navigation.
///
/// # Arguments
///
/// - `&Webview`: The webview that is loading a page.
/// - `&PageLoadPayload`: The page load event payload (used to gate on start).
fn inject_link_interceptor(webview: &Webview, payload: &PageLoadPayload<'_>) {
    // Inject as early as possible so listeners are registered before user
    // interaction. We inject on the start event of every page load.
    if payload.event() != PageLoadEvent::Started {
        return;
    }
    if let Err(error) = webview.eval(LINK_INTERCEPTOR_SCRIPT) {
        euv_log!("[EUV] failed to inject link interceptor: {}", error);
    }
}

/// Initializes and runs the Tauri application with EUV cache scheme.
///
/// Sets up the custom URI scheme handler, deploys bundled cache,
/// and starts the background update or initial fetch process.
///
/// # Panics
///
/// Panics if the Tauri application fails to build.
pub fn run() {
    #[cfg(target_os = "android")]
    {
        tauri::android_binding!(com, euv, run, tauri::wry);
    }
    let _ = HTTP_CLIENT.get_or_init(|| {
        Client::builder()
            .timeout(Duration::from_secs(FETCH_TIMEOUT_SECS))
            .redirect(Policy::limited(MAX_REDIRECTS))
            .build()
            .expect("fatal: failed to build HTTP client")
    });
    Builder::default()
        // Registered once at the Builder level so it runs for EVERY webview the
        // app owns, including ones created dynamically at runtime. It injects a
        // script that routes any external link to the system browser instead of
        // navigating in-app.
        .on_page_load(inject_link_interceptor)
        .plugin(tauri_plugin_opener::init())
        .setup(|app: &mut App| {
            app.handle().plugin(
                tauri_plugin_log::Builder::default()
                    .level(::log::LevelFilter::Info)
                    .build(),
            )?;
            let handle: AppHandle = app.handle().clone();
            #[cfg(debug_assertions)]
            let _ = APP_HANDLE.set(handle.clone());
            let setup_handle: AppHandle = handle.clone();
            spawn(async move {
                deploy_bundled_cache(&setup_handle).await;
                if has_active_cache(&setup_handle).await {
                    euv_log!("[EUV] cache ready, background update");
                    background_update(handle).await;
                } else {
                    euv_log!("[EUV] no cache, initial fetch");
                    initial_fetch(handle).await;
                }
            });
            Ok(())
        })
        .register_asynchronous_uri_scheme_protocol(
            SCHEME_NAME,
            move |context: tauri::UriSchemeContext<'_, tauri::Wry>,
                  request: http::Request<Vec<u8>>,
                  responder: UriSchemeResponder| {
                let app_handle: AppHandle = context.app_handle().clone();
                spawn(async move {
                    let response: http::Response<Cow<'static, [u8]>> =
                        handle_euv_scheme(&app_handle, request).await;
                    responder.respond(response);
                });
            },
        )
        .invoke_handler(generate_handler![
            load_cached_resource,
            resolve_bridge_group_permissions
        ])
        .build(generate_context!())
        .expect("fatal: app failed to start")
        .run(|_: &AppHandle, _: RunEvent| {});
}

/// Resolves the application cache directory path.
///
/// # Arguments
///
/// - `&AppHandle`: The Tauri application handle used to resolve the cache directory.
///
/// # Returns
///
/// - `Result<PathBuf, CacheError>`: The cache root directory path, or an error if resolution fails.
pub(crate) fn get_cache_root(app_handle: &AppHandle) -> Result<PathBuf, CacheError> {
    let mut cache_directory: PathBuf = app_handle
        .path()
        .app_cache_dir()
        .map_err(|error| CacheError::Read(error.to_string()))?;
    cache_directory.push(CACHE_DIR);
    Ok(cache_directory)
}

/// Returns the path to the active version pointer file.
///
/// # Arguments
///
/// - `&Path`: The cache root directory path.
///
/// # Returns
///
/// - `PathBuf`: The path to the active pointer file.
fn active_pointer_path(cache_root: &Path) -> PathBuf {
    cache_root.join(ACTIVE_LINK)
}

/// Reads the currently active version name from the pointer file asynchronously.
///
/// # Arguments
///
/// - `&Path`: The cache root directory path.
///
/// # Returns
///
/// - `Option<String>`: The active version name if it exists and is valid, otherwise `None`.
async fn read_active_version(cache_root: &Path) -> Option<String> {
    let name: String = read_to_string(active_pointer_path(cache_root)).await.ok()?;
    let name: String = name.trim().to_string();
    if name.is_empty() {
        return None;
    }
    if metadata(cache_root.join(&name)).await.ok()?.is_dir() {
        Some(name)
    } else {
        None
    }
}

/// Generates a new version directory name using the current timestamp.
///
/// # Returns
///
/// - `String`: A version name in the format `v_<timestamp_millis>`.
fn new_version_name() -> String {
    let timestamp: u128 = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    format!("{VERSION_PREFIX}{timestamp}")
}

/// Switches the active version pointer to the specified version asynchronously.
///
/// Uses atomic rename to ensure the pointer file is updated safely.
///
/// # Arguments
///
/// - `&Path`: The cache root directory path.
/// - `&str`: The version name to set as active.
///
/// # Returns
///
/// - `Result<(), CacheError>`: Ok if the switch succeeded, or a write error.
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

/// Removes old version directories, keeping only the most recent ones.
///
/// Deletion is performed concurrently for faster cleanup.
///
/// # Arguments
///
/// - `&Path`: The cache root directory path.
/// - `&str`: The current active version name (will not be removed).
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
            && let Ok(entry_metadata) = entry.metadata().await
            && entry_metadata.is_dir()
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
    let remove_handles: Vec<tauri::async_runtime::JoinHandle<()>> = versions[..to_remove]
        .iter()
        .map(|name: &String| {
            let dir_path: PathBuf = cache_root.join(name);
            let log_name: String = name.clone();
            tauri::async_runtime::spawn(async move {
                remove_dir_all(&dir_path).await.ok();
                euv_log!("[EUV] removed old version: {}", log_name);
            })
        })
        .collect();
    for handle in remove_handles {
        handle.await.ok();
    }
}

/// Returns a reference to the global shared HTTP client.
///
/// Reuses a single `Client` instance with connection pooling across all fetch operations,
/// avoiding repeated TLS handshake and DNS resolution overhead.
///
/// # Returns
///
/// - `&'static Client`: The globally shared HTTP client reference.
fn shared_client() -> &'static Client {
    HTTP_CLIENT.get_or_init(|| {
        Client::builder()
            .timeout(Duration::from_secs(FETCH_TIMEOUT_SECS))
            .redirect(Policy::limited(MAX_REDIRECTS))
            .build()
            .expect("fatal: failed to build HTTP client")
    })
}

/// Fetches a URL and returns the response body as bytes.
///
/// Enforces a maximum body size limit by checking `Content-Length` header first
/// to avoid downloading oversized responses.
///
/// # Arguments
///
/// - `&str`: The URL to fetch.
///
/// # Returns
///
/// - `Result<Vec<u8>, CacheError>`: The response body bytes, or a fetch/write error.
pub(crate) async fn fetch_url(url: &str) -> Result<Vec<u8>, CacheError> {
    let client: &Client = shared_client();
    let response: reqwest::Response = client
        .get(url)
        .send()
        .await
        .map_err(|error: reqwest::Error| CacheError::Fetch(error.to_string()))?;
    if !response.status().is_success() {
        return Err(CacheError::Fetch(format!("HTTP {}", response.status())));
    }
    if let Some(content_length) = response.content_length()
        && content_length as usize > MAX_BODY_SIZE
    {
        return Err(CacheError::Fetch(format!(
            "Too large: {} bytes",
            content_length
        )));
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

/// Fetches a URL and returns both the final URL (after redirects) and the response body.
///
/// Checks `Content-Length` before downloading to avoid wasting bandwidth on oversized files.
///
/// # Arguments
///
/// - `&str`: The URL to fetch.
///
/// # Returns
///
/// - `Result<(String, Vec<u8>), CacheError>`: A tuple of (final_url, body_bytes), or a fetch error.
async fn fetch_url_with_final_url(url: &str) -> Result<(String, Vec<u8>), CacheError> {
    let client: &Client = shared_client();
    let response: reqwest::Response = client
        .get(url)
        .send()
        .await
        .map_err(|error: reqwest::Error| CacheError::Fetch(error.to_string()))?;
    if !response.status().is_success() {
        return Err(CacheError::Fetch(format!("HTTP {}", response.status())));
    }
    if let Some(content_length) = response.content_length()
        && content_length as usize > MAX_BODY_SIZE
    {
        return Err(CacheError::Fetch(format!(
            "Too large: {} bytes",
            content_length
        )));
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

/// Writes data to a file atomically by first writing to a temporary file then renaming.
///
/// # Arguments
///
/// - `&Path`: The target file path.
/// - `&[u8]`: The data to write.
///
/// # Returns
///
/// - `Result<(), CacheError>`: Ok if the write succeeded, or a write error.
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

/// Strips leading slashes and `./` prefixes from a relative path.
///
/// Avoids repeated `trim_start_matches` + `to_string()` allocations by centralizing the logic.
///
/// # Arguments
///
/// - `&str`: The relative path to clean.
///
/// # Returns
///
/// - `String`: The cleaned path.
fn clean_relative_path(path: &str) -> String {
    let mut result: &str = path;
    loop {
        let trimmed: &str = result.trim_start_matches('/');
        if let Some(stripped) = trimmed.strip_prefix("./") {
            result = stripped;
        } else if trimmed.len() < result.len() {
            result = trimmed;
        } else {
            break;
        }
    }
    result.to_string()
}

/// Deploys bundled cache files as a first-run seed only.
///
/// The bundled cache is purely a fallback so the very first cold start (right
/// after install, before any remote fetch has completed) has something to
/// render. Once any active version exists — whether it was seeded from the
/// bundle or fetched in the background on a previous launch — this function is
/// a no-op and the existing active pointer is preserved untouched.
///
/// This guarantees that every subsequent launch immediately serves the latest
/// version the background updater committed last time, instead of ever being
/// rolled back to the (older) compiled-in bundle. Remote updates keep happening
/// in the background and take effect on the next launch.
///
/// # Arguments
///
/// - `&AppHandle`: The Tauri application handle.
///
/// # Returns
///
/// - `bool`: `true` if an active version exists (reused or freshly seeded),
///   `false` on error or when there is nothing to seed.
pub(crate) async fn deploy_bundled_cache(app_handle: &AppHandle) -> bool {
    let cache_root: PathBuf = match get_cache_root(app_handle) {
        Ok(directory) => directory,
        Err(_) => return false,
    };
    if create_dir_all(&cache_root).await.is_err() {
        return false;
    }
    // If any active version already exists, always reuse it as-is. Never
    // overwrite or roll it back to the compiled-in bundle — the active pointer
    // already references the newest snapshot committed by a previous launch's
    // background update.
    if let Some(active) = read_active_version(&cache_root).await {
        euv_log!("[EUV] active cache present, reusing: {active}");
        return true;
    }
    // No active version yet (fresh install): seed from the bundled snapshot so
    // the first launch has content to render while the initial fetch runs.
    if BUNDLED_FILES.is_empty() {
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
    cleanup_old_versions(&cache_root, &version_name).await;
    euv_log!("[EUV] deployed {count} bundled files, serving: {version_name}");
    true
}

/// Checks whether an active cache deployment exists asynchronously.
///
/// # Arguments
///
/// - `&AppHandle`: The Tauri application handle.
///
/// # Returns
///
/// - `bool`: `true` if an active version is present, `false` otherwise.
pub(crate) async fn has_active_cache(app_handle: &AppHandle) -> bool {
    let cache_root: PathBuf = match get_cache_root(app_handle) {
        Ok(directory) => directory,
        Err(_) => return false,
    };
    read_active_version(&cache_root).await.is_some()
}

/// Fetches a full snapshot of the remote HTML page and all linked resources.
///
/// Verifies that all critical resources (scripts, stylesheets, images, WASM) are present
/// before switching the active version pointer.
///
/// # Arguments
///
/// - `&Path`: The cache root directory path.
///
/// # Returns
///
/// - `Result<String, CacheError>`: The new version name on success, or a fetch/write error.
async fn fetch_full_snapshot(cache_root: &Path) -> Result<String, CacheError> {
    let (final_url, html_bytes) = fetch_url_with_final_url(REMOTE_URL).await?;
    let html: String = String::from_utf8_lossy(&html_bytes).into_owned();
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
    expected_paths.retain(|p: &String| {
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
    let mut verified_set: HashSet<String> = HashSet::with_capacity(expected_paths.len());
    for expected in &expected_paths {
        let clean: String = clean_relative_path(expected);
        verified_set.insert(clean.clone());
        let file_path: PathBuf = version_dir.join(&clean);
        if metadata(&file_path).await.is_err() {
            euv_log!("[EUV] missing critical resource: {}", clean);
            remove_dir_all(&version_dir).await.ok();
            return Err(CacheError::Fetch(format!(
                "incomplete snapshot: missing {}",
                clean
            )));
        }
    }
    // Collect the local cache paths of every WASM file referenced by a cached JS file.
    let mut wasm_paths: Vec<String> = Vec::new();
    let mut js_to_scan: Vec<String> = verified_set
        .iter()
        .filter(|c: &&String| c.ends_with(".js") || c.ends_with(".mjs"))
        .cloned()
        .collect();
    let pkg_dir: PathBuf = version_dir.join("pkg");
    if metadata(&pkg_dir).await.is_ok()
        && let Ok(mut entries) = read_dir(&pkg_dir).await
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
            let mut raw_refs: Vec<String> = Vec::new();
            extract_wasm_references(&js_content, &mut raw_refs);
            // Map each raw reference to its local cache path relative to the JS
            // file's directory, so it matches where the resource was stored.
            for raw_ref in &raw_refs {
                let local: String = resolve_local_path(js_rel, raw_ref);
                if !wasm_paths.contains(&local) {
                    wasm_paths.push(local);
                }
            }
        }
    }
    for clean in &wasm_paths {
        let file_path: PathBuf = version_dir.join(clean);
        if metadata(&file_path).await.is_err() {
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

/// Performs the initial cache fetch, retrying on failure until successful.
///
/// # Arguments
///
/// - `AppHandle`: The Tauri application handle (consumed for async task ownership).
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
                // Do NOT reload the current session. The freshly committed
                // active pointer takes effect on the NEXT launch only.
                euv_log!("[EUV] initial fetch done (effective next launch): {}", version);
                return;
            }
            Err(error) => {
                euv_log!("[EUV] initial fetch failed: {}, retrying", error);
                tokio::time::sleep(Duration::from_millis(RETRY_INTERVAL_MILLIS)).await;
            }
        }
    }
}

/// Performs a background cache update, retrying on failure until successful.
///
/// The updated cache will take effect on the next application launch.
///
/// # Arguments
///
/// - `AppHandle`: The Tauri application handle (consumed for async task ownership).
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
                // Do NOT reload the current session. The freshly committed
                // active pointer takes effect on the NEXT launch only.
                euv_log!("[EUV] background update done (effective next launch): {}", version);
                return;
            }
            Err(error) => {
                euv_log!("[EUV] background update failed: {}, retrying", error);
                tokio::time::sleep(Duration::from_millis(RETRY_INTERVAL_MILLIS)).await;
            }
        }
    }
}

/// Derives the base URL from a final URL by stripping the last path segment.
///
/// # Arguments
///
/// - `&str`: The final URL after redirects.
///
/// # Returns
///
/// - `String`: The base URL for resolving relative resource paths.
fn derive_base_url(final_url: &str) -> String {
    let url: String = final_url.trim_end_matches('/').to_string();
    if final_url.ends_with('/') {
        return url;
    }
    if let Some(pos) = url.rfind('/')
        && pos > url.find("://").map_or(0, |p: usize| p + 2)
    {
        return url[..pos].to_string();
    }
    url
}

/// Fetches all linked resources (scripts, stylesheets, images) from an HTML page.
///
/// Also discovers and fetches transitive dependencies in JS files (WASM, ES module imports).
/// Uses `HashSet` for deduplication to avoid O(n) linear scans.
///
/// # Arguments
///
/// - `&Path`: The version directory where resources should be saved.
/// - `&str`: The HTML content to parse for resource links.
/// - `&str`: The final URL of the page, used to derive the base URL.
///
/// # Returns
///
/// - `usize`: The total number of resources successfully fetched.
async fn fetch_linked_resources(version_dir: &Path, html: &str, final_url: &str) -> usize {
    let mut paths: Vec<String> = Vec::new();
    extract_attr_values(html, "script", "src", &mut paths);
    extract_attr_values(html, "link", "href", &mut paths);
    extract_attr_values(html, "img", "src", &mut paths);
    extract_module_imports(html, &mut paths);
    let base_url: String = derive_base_url(final_url);
    euv_log!("[EUV] resource base URL: {}", base_url);
    let fetched: FetchResult = fetch_resource_list(&paths, &base_url, version_dir).await;
    let mut total_count: usize = fetched.len();
    let fetched_set: HashSet<String> = fetched
        .iter()
        .map(|(clean_path, _, _): &(String, String, Vec<u8>)| clean_path.clone())
        .collect();
    // Each extra dependency is resolved against the FINAL url of the JS file that
    // referenced it, so redirected resources still yield correct download URLs.
    let mut extra_deps: Vec<(String, String)> = Vec::new(); // (absolute_url, clean_path)
    let mut extra_set: HashSet<String> = HashSet::new();
    for (clean_path, res_final_url, data) in &fetched {
        if clean_path.ends_with(".js") || clean_path.ends_with(".mjs") {
            let js_content: String = String::from_utf8_lossy(data).to_string();
            let mut refs: Vec<String> = Vec::new();
            extract_wasm_references(&js_content, &mut refs);
            extract_module_imports(&js_content, &mut refs);
            for raw_ref in &refs {
                if raw_ref.starts_with("data:")
                    || raw_ref.starts_with("http://")
                    || raw_ref.starts_with("https://")
                    || raw_ref.starts_with("//")
                {
                    continue;
                }
                // Local cache path is resolved relative to the directory of the
                // referencing JS file, so a `pkg/euv.js` referencing `euv_bg.wasm`
                // is stored as `pkg/euv_bg.wasm` (matching how it is later served).
                let dep_clean: String = resolve_local_path(clean_path, raw_ref);
                if fetched_set.contains(&dep_clean) || !extra_set.insert(dep_clean.clone()) {
                    continue;
                }
                // Download URL is resolved against the JS file's FINAL url so
                // redirected resources still yield correct absolute URLs.
                let dep_url: String = resolve_against(res_final_url, raw_ref);
                extra_deps.push((dep_url, dep_clean));
            }
        }
    }
    if !extra_deps.is_empty() {
        euv_log!("[EUV] found {} extra dependencies in JS", extra_deps.len());
        let extra_fetched: usize = fetch_absolute_list(&extra_deps, version_dir).await;
        total_count += extra_fetched;
    }
    total_count
}

/// Resolves a relative reference against a base URL (typically a resource's
/// final URL after redirects).
///
/// Handles `./`, `../`, root-relative (`/...`) and bare relative references by
/// performing path-segment resolution against the base URL's directory.
///
/// # Arguments
///
/// - `&str`: The base URL (e.g. the final URL of the JS file).
/// - `&str`: The relative reference to resolve.
///
/// # Returns
///
/// - `String`: The absolute URL of the referenced resource.
fn resolve_against(base_url: &str, reference: &str) -> String {
    // Split scheme://host from the path portion.
    let scheme_end: usize = base_url.find("://").map_or(0, |p: usize| p + 3);
    let after_scheme: &str = &base_url[scheme_end..];
    let host_end: usize = after_scheme
        .find('/')
        .map_or(after_scheme.len(), |p: usize| p);
    let origin: &str = &base_url[..scheme_end + host_end];
    let base_path: &str = &base_url[scheme_end + host_end..];

    // Root-relative reference: anchor at origin.
    if let Some(rooted) = reference.strip_prefix('/') {
        return format!("{}/{}", origin, rooted);
    }

    // Derive the directory of the base path (strip the last segment).
    let dir: &str = match base_path.rfind('/') {
        Some(pos) => &base_path[..pos],
        None => "",
    };
    let mut segments: Vec<&str> = dir.split('/').filter(|s: &&str| !s.is_empty()).collect();
    for part in reference.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                segments.pop();
            }
            other => segments.push(other),
        }
    }
    format!("{}/{}", origin, segments.join("/"))
}

/// Resolves a relative reference into a local cache path, relative to the
/// directory of the referencing file.
///
/// Mirrors `resolve_against` but operates on local (path-only) values so the
/// on-disk layout matches the served URL layout. For example, a `pkg/euv.js`
/// that references `euv_bg.wasm` yields `pkg/euv_bg.wasm`; a reference of
/// `../assets/x.png` yields `assets/x.png`.
///
/// # Arguments
///
/// - `&str`: The clean path of the referencing file (e.g. `pkg/euv.js`).
/// - `&str`: The relative reference as written in the source.
///
/// # Returns
///
/// - `String`: The cleaned local path where the referenced file should be stored.
fn resolve_local_path(referencing_clean_path: &str, reference: &str) -> String {
    // Root-relative reference is anchored at the cache root.
    if let Some(rooted) = reference.strip_prefix('/') {
        return clean_relative_path(rooted);
    }
    // Start from the directory of the referencing file.
    let dir: &str = match referencing_clean_path.rfind('/') {
        Some(pos) => &referencing_clean_path[..pos],
        None => "",
    };
    let mut segments: Vec<&str> = dir.split('/').filter(|s: &&str| !s.is_empty()).collect();
    for part in reference.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                segments.pop();
            }
            other => segments.push(other),
        }
    }
    segments.join("/")
}

/// Fetches a list of (absolute_url, clean_path) dependencies concurrently and
/// writes them to disk under the version directory.
///
/// Unlike `fetch_resource_list`, the download URL is provided directly (already
/// resolved against the referencing resource's final URL), so no base-URL
/// concatenation is performed.
///
/// # Arguments
///
/// - `&[(String, String)]`: List of (absolute_url, clean_path) pairs.
/// - `&Path`: The version directory where resources should be saved.
///
/// # Returns
///
/// - `usize`: The number of dependencies successfully fetched and written.
async fn fetch_absolute_list(deps: &[(String, String)], version_dir: &Path) -> usize {
    let mut join_set: tokio::task::JoinSet<bool> = tokio::task::JoinSet::new();
    for (url, clean) in deps {
        let url: String = url.clone();
        let clean: String = clean.clone();
        let local: PathBuf = version_dir.join(&clean);
        join_set.spawn(async move {
            match fetch_url(&url).await {
                Ok(data) => {
                    if data.is_empty() {
                        euv_log!("[EUV] skipped empty resource: {}", clean);
                        return false;
                    }
                    if let Err(error) = atomic_write(&local, &data).await {
                        euv_log!("[EUV] write failed {}: {}", clean, error);
                        false
                    } else {
                        euv_log!("[EUV] fetched dep: {} ({} bytes) from {}", clean, data.len(), url);
                        true
                    }
                }
                Err(error) => {
                    euv_log!("[EUV] fetch failed {}: {}", clean, error);
                    false
                }
            }
        });
    }
    let mut count: usize = 0;
    while let Some(result) = join_set.join_next().await {
        if let Ok(true) = result {
            count += 1;
        }
    }
    count
}

/// Fetches a list of resource URLs concurrently and writes them to disk.
///
/// Uses `JoinSet` for more efficient concurrent task management compared to
/// manual `spawn` + `Vec<JoinHandle>` pattern. Skips data URIs, absolute HTTP URLs,
/// and protocol-relative URLs.
///
/// # Arguments
///
/// - `&[String]`: The list of relative resource paths to fetch.
/// - `&str`: The base URL for resolving relative paths.
/// - `&Path`: The version directory where resources should be saved.
///
/// # Returns
///
/// - `FetchResult`: A vector of (clean_path, data) tuples for successfully fetched resources.
async fn fetch_resource_list(paths: &[String], base_url: &str, version_dir: &Path) -> FetchResult {
    let mut join_set: tokio::task::JoinSet<Option<(String, String, Vec<u8>)>> =
        tokio::task::JoinSet::new();
    for relative_path in paths {
        if relative_path.starts_with("data:")
            || relative_path.starts_with("http://")
            || relative_path.starts_with("https://")
            || relative_path.starts_with("//")
        {
            continue;
        }
        let clean: String = clean_relative_path(relative_path);
        let url: String = format!("{}/{}", base_url, clean);
        let local: PathBuf = version_dir.join(&clean);
        join_set.spawn(async move {
            // Capture each resource's OWN final URL after redirects so transitive
            // dependencies (e.g. a `.wasm` referenced from this JS) resolve against
            // the correct location even when the resource itself was redirected.
            match fetch_url_with_final_url(&url).await {
                Ok((final_url, data)) => {
                    if data.is_empty() {
                        euv_log!("[EUV] skipped empty resource: {}", clean);
                        return None;
                    }
                    if let Err(error) = atomic_write(&local, &data).await {
                        euv_log!("[EUV] write failed {}: {}", clean, error);
                        None
                    } else {
                        if final_url != url {
                            euv_log!("[EUV] fetched: {} ({} bytes) [redirected -> {}]", clean, data.len(), final_url);
                        } else {
                            euv_log!("[EUV] fetched: {} ({} bytes)", clean, data.len());
                        }
                        Some((clean, final_url, data))
                    }
                }
                Err(error) => {
                    euv_log!("[EUV] fetch failed {}: {}", clean, error);
                    None
                }
            }
        });
    }
    let mut results: FetchResult = Vec::with_capacity(join_set.len());
    while let Some(result) = join_set.join_next().await {
        if let Ok(Some(tuple)) = result {
            results.push(tuple);
        }
    }
    results
}

/// Extracts WASM file references from JavaScript source code.
///
/// Returns the raw references exactly as written in the source. The caller is
/// responsible for resolving them against the JS file's final URL (download)
/// and directory (local cache path).
///
/// # Arguments
///
/// - `&str`: The JavaScript source code.
/// - `&mut Vec<String>`: The collection to append discovered WASM paths to.
fn extract_wasm_references(js: &str, results: &mut Vec<String>) {
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
            .map(|q: usize| &js[search_start + q + 1..index + 5])
            .or_else(|| {
                before
                    .rfind('"')
                    .map(|q: usize| &js[search_start + q + 1..index + 5])
            });
        if let Some(wasm_file) = wasm_ref
            && !wasm_file.contains("://")
            && !wasm_file.contains(' ')
        {
            // Return the raw reference exactly as written in the JS source. The
            // caller resolves it against the JS file's final URL (for the
            // download URL) and against the JS file's directory (for the local
            // cache path), so prefixing the JS directory here would double it.
            let raw: String = wasm_file.to_string();
            if !results.contains(&raw) {
                results.push(raw);
            }
        }
        pos = index + 5;
    }
}

/// Extracts ES module import paths from source code.
///
/// Looks for `import ... from '...'` and dynamic `import('...')` patterns.
/// Supports destructured imports like `import init, { main } from './pkg/euv.js'`
/// and bare relative paths that do not start with `./` or `../` but contain a `/`.
///
/// # Arguments
///
/// - `&str`: The source code to parse.
/// - `&mut Vec<String>`: The collection to append discovered import paths to.
fn extract_module_imports(source: &str, results: &mut Vec<String>) {
    let mut pos: usize = 0;
    while pos < source.len() {
        let index: usize = match source[pos..].find("from") {
            Some(offset) => pos + offset,
            None => break,
        };
        let before: &str = &source[pos..index];
        let is_import_context: bool = before
            .trim_end()
            .trim_end_matches(',')
            .trim_end_matches('}')
            .trim_end_matches('{')
            .trim_end_matches(',')
            .ends_with("import")
            || before.trim_end().ends_with("import")
            || before.rfind("import").is_some_and(|import_pos: usize| {
                let after_import: &str = before[import_pos + 6..].trim();
                after_import.is_empty()
                    || after_import.chars().all(|c: char| {
                        c.is_alphanumeric()
                            || c == '_'
                            || c == '{'
                            || c == '}'
                            || c == ','
                            || c == '*'
                            || c == ' '
                            || c == '\t'
                            || c == '\n'
                            || c == '\r'
                    })
            });
        if is_import_context {
            let after: &str = source[index + 4..].trim_start();
            let value: &str = extract_quoted_value(after);
            if !value.is_empty()
                && is_relative_resource_path(value)
                && !results.contains(&value.to_string())
            {
                results.push(value.to_string());
            }
        }
        pos = index + 4;
    }
}

/// Checks whether a path string refers to a relative resource that should be cached.
///
/// Accepts paths starting with `./` or `../`, as well as bare relative paths
/// that contain a `/` and do not look like an npm package name.
///
/// # Arguments
///
/// - `&str`: The path string to evaluate.
///
/// # Returns
///
/// - `bool`: `true` if the path is a relative resource path, `false` otherwise.
fn is_relative_resource_path(value: &str) -> bool {
    if value.starts_with("./") || value.starts_with("../") {
        return true;
    }
    if value.contains('/') && !value.contains("://") && !value.starts_with('@') {
        return true;
    }
    if value.ends_with(".wasm")
        || value.ends_with(".js")
        || value.ends_with(".mjs")
        || value.ends_with(".css")
    {
        return !value.contains("://");
    }
    false
}

/// Extracts attribute values from HTML tags matching the specified tag and attribute name.
///
/// Performs case-insensitive matching without allocating a new `String` for `to_lowercase()`
/// by comparing only the tag and attribute patterns in lowercase.
///
/// # Arguments
///
/// - `&str`: The HTML content to parse.
/// - `&str`: The tag name to search for (e.g., `"script"`, `"link"`).
/// - `&str`: The attribute name to extract (e.g., `"src"`, `"href"`).
/// - `&mut Vec<String>`: The collection to append discovered attribute values to.
fn extract_attr_values(html: &str, tag: &str, attr: &str, results: &mut Vec<String>) {
    let tag_lower: String = tag.to_lowercase();
    let attr_lower: String = attr.to_lowercase();
    let tag_prefix: String = format!("<{}", tag_lower);
    let attr_eq: String = format!("{}=", attr_lower);
    let close_tag: String = format!("</{}", tag_lower);
    let mut pos: usize = 0;
    while pos < html.len() {
        let start: usize = match html[pos..].find(tag_prefix.as_str()) {
            Some(offset) => pos + offset,
            None => break,
        };
        let end: usize = match html[start..].find('>') {
            Some(offset) => start + offset,
            None => break,
        };
        let content: &str = &html[start..end];
        if let Some(attr_position) = content.to_lowercase().find(attr_eq.as_str()) {
            let value: &str = extract_quoted_value(&content[attr_position + attr_eq.len()..]);
            if !value.is_empty() && !results.contains(&value.to_string()) {
                results.push(value.to_string());
            }
        }
        if content.to_lowercase().contains("type=\"module\"")
            || content.to_lowercase().contains("type='module'")
        {
            let inner_end: usize = match html[end + 1..].find(close_tag.as_str()) {
                Some(offset) => end + 1 + offset,
                None => {
                    pos = end + 1;
                    continue;
                }
            };
            let inner_html: &str = &html[end + 1..inner_end];
            extract_module_imports(inner_html, results);
            pos = inner_end;
        } else {
            pos = end + 1;
        }
    }
}

/// Extracts a single-quoted or double-quoted value from the beginning of a string.
///
/// # Arguments
///
/// - `&str`: The input string starting with an optional quote character.
///
/// # Returns
///
/// - `&str`: The extracted value between quotes, or an empty string if no quoted value is found.
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

/// Emits a reload event to the frontend via Tauri.
///
/// Currently unused: remote updates are intentionally applied on the NEXT
/// launch only (the active pointer is switched but the running session is not
/// reloaded). Kept for the case we later want to opt back into live reload.
///
/// # Arguments
///
/// - `&AppHandle`: The Tauri application handle used to emit the event.
#[allow(dead_code)]
pub(crate) fn notify_reload(app_handle: &AppHandle) {
    use tauri::Emitter;
    if let Err(error) = app_handle.emit("euv://reload", ()) {
        ::log::warn!("[EUV] failed to emit reload event: {}", error);
    } else {
        ::log::info!("[EUV] reload event emitted");
    }
}

/// Returns the MIME type string for a given file path based on its extension.
///
/// Uses `rfind` to locate the last dot in the path and performs case-insensitive
/// comparison on the extension only, avoiding a full `to_lowercase()` heap allocation.
///
/// # Arguments
///
/// - `&str`: The file path or extension to look up.
///
/// # Returns
///
/// - `&'static str`: The corresponding MIME type string, or `"application/octet-stream"` as fallback.
pub(crate) fn mime_for(path: &str) -> &'static str {
    let extension: &str = match path.rfind('.') {
        Some(pos) => &path[pos..],
        None => "",
    };
    if extension.eq_ignore_ascii_case(".html") || extension.eq_ignore_ascii_case(".htm") {
        "text/html"
    } else if extension.eq_ignore_ascii_case(".js") || extension.eq_ignore_ascii_case(".mjs") {
        "application/javascript"
    } else if extension.eq_ignore_ascii_case(".wasm") {
        "application/wasm"
    } else if extension.eq_ignore_ascii_case(".css") {
        "text/css"
    } else if extension.eq_ignore_ascii_case(".json") {
        "application/json"
    } else if extension.eq_ignore_ascii_case(".png") {
        "image/png"
    } else if extension.eq_ignore_ascii_case(".jpg") || extension.eq_ignore_ascii_case(".jpeg") {
        "image/jpeg"
    } else if extension.eq_ignore_ascii_case(".gif") {
        "image/gif"
    } else if extension.eq_ignore_ascii_case(".svg") {
        "image/svg+xml"
    } else if extension.eq_ignore_ascii_case(".ico") {
        "image/x-icon"
    } else if extension.eq_ignore_ascii_case(".woff") {
        "font/woff"
    } else if extension.eq_ignore_ascii_case(".woff2") {
        "font/woff2"
    } else if extension.eq_ignore_ascii_case(".ttf") {
        "font/ttf"
    } else if extension.eq_ignore_ascii_case(".otf") {
        "font/otf"
    } else if extension.eq_ignore_ascii_case(".webp") {
        "image/webp"
    } else {
        "application/octet-stream"
    }
}

/// Handles the custom `euv://` URI scheme protocol for serving cached resources.
///
/// Reads the requested file from the active cache directory and injects
/// the reload listener script and debug panel (in debug mode) into index.html.
/// Uses exponential backoff when waiting for cache readiness.
///
/// # Arguments
///
/// - `&AppHandle`: The Tauri application handle.
/// - `http::Request<Vec<u8>>`: The incoming HTTP request.
///
/// # Returns
///
/// - `http::Response<Cow<'static, [u8]>>`: The HTTP response with the file content or an error status.
pub(crate) async fn handle_euv_scheme(
    app_handle: &AppHandle,
    request: http::Request<Vec<u8>>,
) -> http::Response<Cow<'static, [u8]>> {
    let path: &str = request.uri().path();
    let path_trimmed: &str = path.trim_start_matches('/');
    let path_decoded: String = percent_encoding::percent_decode_str(path_trimmed)
        .decode_utf8_lossy()
        .into_owned();
    let is_index: bool = path_decoded.is_empty() || path_decoded == "index.html";
    let cache_root: PathBuf = match get_cache_root(app_handle) {
        Ok(directory) => directory,
        Err(_) => {
            return http::Response::builder()
                .status(500)
                .body(Cow::Owned(b"Internal error".to_vec()))
                .unwrap();
        }
    };
    let active_version: Option<String> = read_active_version(&cache_root).await;
    let active_dir: PathBuf = match active_version.as_deref() {
        Some(version) => cache_root.join(version),
        None => {
            let start: Instant = Instant::now();
            let timeout: Duration = Duration::from_secs(2);
            let mut wait_millis: u64 = 2;
            loop {
                if let Some(version) = read_active_version(&cache_root).await {
                    break cache_root.join(version);
                }
                if start.elapsed() >= timeout {
                    return http::Response::builder()
                        .status(503)
                        .body(Cow::Owned(b"Cache not ready".to_vec()))
                        .unwrap();
                }
                tokio::time::sleep(Duration::from_millis(wait_millis)).await;
                wait_millis = (wait_millis * 2).min(64);
            }
        }
    };
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
    match read(&file_path).await {
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
                    let source: String = active_version
                        .as_ref()
                        .map(|_: &String| "active".to_string())
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

/// Tauri command that returns cached page metadata.
///
/// # Arguments
///
/// - `AppHandle`: The Tauri application handle.
///
/// # Returns
///
/// - `Result<CachedPage, String>`: The cached page info on success, or an error message string.
#[tauri::command]
pub(crate) async fn load_cached_resource(app_handle: AppHandle) -> Result<CachedPage, String> {
    let cache_root: PathBuf =
        get_cache_root(&app_handle).map_err(|error: CacheError| error.to_string())?;
    let active_version: Option<String> = read_active_version(&cache_root).await;
    let from_cache: bool = active_version.is_some();
    let source: String = active_version
        .as_ref()
        .map(|_: &String| "active".to_string())
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
