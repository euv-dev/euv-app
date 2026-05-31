use super::*;

use tauri::Manager;

/// Reads the cached HTML content from the local file system.
///
/// # Arguments
/// - `app_handle`: The Tauri application handle used to resolve the app data directory.
///
/// # Returns
/// - `Result<String, CacheError>`: The cached HTML string on success, or a `CacheError` on failure.
pub async fn read_cache(app_handle: &tauri::AppHandle) -> Result<String, CacheError> {
    let cache_dir: std::path::PathBuf = app_handle.path().app_data_dir().map_err(|error: tauri::Error| CacheError::Read(format!("{}", error)))?;
    let cache_path: std::path::PathBuf = cache_dir.join(CACHE_FILENAME);
    if cache_path.exists() {
        let content: String = tokio::fs::read_to_string(&cache_path).await.map_err(|error: std::io::Error| CacheError::Read(format!("{}", error)))?;
        log::info!("[cache] Read cached content, size: {} bytes", content.len());
        return Ok(content);
    }
    log::info!("[cache] No cached file found at {:?}", cache_path);
    Err(CacheError::Read("Cache file not found".to_string()))
}

/// Writes the HTML content to the local cache file.
///
/// # Arguments
/// - `app_handle`: The Tauri application handle used to resolve the app data directory.
/// - `content`: The HTML content string to cache.
///
/// # Returns
/// - `Result<(), CacheError>`: Ok on success, or a `CacheError` on failure.
pub async fn write_cache(app_handle: &tauri::AppHandle, content: &str) -> Result<(), CacheError> {
    let cache_dir: std::path::PathBuf = app_handle.path().app_data_dir().map_err(|error: tauri::Error| CacheError::Write(format!("{}", error)))?;
    tokio::fs::create_dir_all(&cache_dir).await.map_err(|error: std::io::Error| CacheError::Write(format!("{}", error)))?;
    let cache_path: std::path::PathBuf = cache_dir.join(CACHE_FILENAME);
    tokio::fs::write(&cache_path, content).await.map_err(|error: std::io::Error| CacheError::Write(format!("{}", error)))?;
    log::info!("[cache] Cached content written, size: {} bytes", content.len());
    Ok(())
}

/// Fetches the remote HTML content from the configured URL.
///
/// # Returns
/// - `Result<String, CacheError>`: The fetched HTML string on success, or a `CacheError` on failure.
pub async fn fetch_remote() -> Result<String, CacheError> {
    let client: reqwest::Client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(FETCH_TIMEOUT_SECS))
        .build()
        .map_err(|error: reqwest::Error| CacheError::Fetch(format!("{}", error)))?;
    let response: reqwest::Response = client.get(REMOTE_URL).send().await.map_err(|error: reqwest::Error| CacheError::Fetch(format!("{}", error)))?;
    let status: reqwest::StatusCode = response.status();
    if !status.is_success() {
        return Err(CacheError::Fetch(format!("HTTP status: {}", status)));
    }
    let content: String = response.text().await.map_err(|error: reqwest::Error| CacheError::Fetch(format!("{}", error)))?;
    log::info!("[cache] Fetched remote content, size: {} bytes", content.len());
    Ok(content)
}

/// Loads the resource content: first tries the local cache, then falls back to a remote fetch.
///
/// # Arguments
/// - `app_handle`: The Tauri application handle used to resolve the app data directory.
///
/// # Returns
/// - `Result<LoadResult, CacheError>`: A `LoadResult` containing the content and its source.
pub async fn load_resource(app_handle: &tauri::AppHandle) -> Result<LoadResult, CacheError> {
    match read_cache(app_handle).await {
        Ok(content) => Ok(LoadResult { content, from_cache: true }),
        Err(_) => {
            log::info!("[cache] Cache miss, fetching from remote...");
            let content: String = fetch_remote().await?;
            let _ = write_cache(app_handle, &content).await;
            Ok(LoadResult { content, from_cache: false })
        }
    }
}

/// Asynchronously updates the local cache by fetching the latest remote content.
/// This is intended to be called after app startup so the next launch uses fresh content.
///
/// # Arguments
/// - `app_handle`: The Tauri application handle used to resolve the app data directory.
pub async fn update_cache_async(app_handle: tauri::AppHandle) {
    log::info!("[cache] Starting async cache update...");
    match fetch_remote().await {
        Ok(content) => {
            if let Err(error) = write_cache(&app_handle, &content).await {
                log::error!("[cache] Failed to write updated cache: {}", error);
            } else {
                log::info!("[cache] Async cache update completed successfully");
            }
        }
        Err(error) => {
            log::error!("[cache] Failed to fetch remote content for update: {}", error);
        }
    }
}

/// Tauri command that loads the cached or remote resource content.
///
/// # Arguments
/// - `app_handle`: The Tauri application handle used to resolve the app data directory.
///
/// # Returns
/// - `Result<LoadResult, String>`: A `LoadResult` containing the content and its source, or an error message.
#[tauri::command]
pub async fn load_cached_resource(app_handle: tauri::AppHandle) -> Result<LoadResult, String> {
    load_resource(&app_handle).await.map_err(|error: CacheError| format!("{}", error))
}
