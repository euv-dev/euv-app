/// URL resolution and filesystem path mapping utilities.
use std::path::PathBuf;

/// Maps a URL to a local filesystem path within the cache directory.
pub fn url_to_cache_path(cache_dir: &std::path::Path, url: &str) -> Option<PathBuf> {
    let parsed = match url::Url::parse(url) {
        Ok(p) => p,
        Err(_) => return None,
    };
    let host = parsed.host_str().unwrap_or("unknown");
    let path = parsed.path();

    let rel_path = if path == "/" || path.is_empty() {
        PathBuf::from(host).join("index.html")
    } else {
        let clean_path = path.strip_prefix('/').unwrap_or(path);
        if clean_path.is_empty() {
            PathBuf::from(host).join("index.html")
        } else {
            PathBuf::from(host).join(clean_path)
        }
    };

    Some(cache_dir.join(rel_path))
}

/// Extracts all resource URLs from an HTML document.
pub fn extract_resource_urls(html: &str, base_url: &str) -> Vec<(String, String)> {
    let mut resources = Vec::new();
    let base = match url::Url::parse(base_url) {
        Ok(b) => b,
        Err(_) => return resources,
    };

    let prefixes: Vec<(&str, &str, &str)> = vec![
        ("href=\"", "\"", "link"),
        ("href='", "'", "link"),
        ("src=\"", "\"", "script"),
        ("src='", "'", "script"),
    ];

    for (prefix, suffix, kind) in &prefixes {
        let mut search_start = 0;
        while let Some(pos) = html[search_start..].find(*prefix) {
            let abs_pos = search_start + pos + prefix.len();
            if let Some(end_pos) = html[abs_pos..].find(*suffix) {
                let raw_url = &html[abs_pos..abs_pos + end_pos];
                if !raw_url.is_empty()
                    && !raw_url.starts_with("data:")
                    && !raw_url.starts_with('#')
                    && !raw_url.starts_with("javascript:")
                {
                    if let Ok(resolved) = base.join(raw_url) {
                        resources.push((resolved.as_str().to_string(), kind.to_string()));
                    }
                }
                search_start = abs_pos + end_pos + suffix.len();
            } else {
                break;
            }
        }
    }

    resources.dedup();
    resources
}

/// Rewrites HTML content, replacing remote URLs with local file:// paths.
pub fn rewrite_html_urls(html: &str, base_url: &str, cache_dir: &std::path::Path) -> String {
    let mut result = html.to_string();
    let base = match url::Url::parse(base_url) {
        Ok(b) => b,
        Err(_) => return result,
    };

    let mut replacements: Vec<(String, String)> = Vec::new();

    // Find all href="..." and src="..."
    let attr_prefixes: Vec<(&str, &str)> = vec![
        ("href=\"", "\""),
        ("href='", "'"),
        ("src=\"", "\""),
        ("src='", "'"),
    ];

    for (prefix, suffix) in &attr_prefixes {
        let mut search_start = 0;
        while let Some(pos) = result[search_start..].find(*prefix) {
            let abs_pos = search_start + pos + prefix.len();
            if let Some(end_pos) = result[abs_pos..].find(*suffix) {
                let raw_url = &result[abs_pos..abs_pos + end_pos];
                if !raw_url.is_empty()
                    && !raw_url.starts_with("data:")
                    && !raw_url.starts_with('#')
                    && !raw_url.starts_with("javascript:")
                    && !raw_url.starts_with("file:")
                {
                    if let Ok(resolved) = base.join(raw_url) {
                        if let Some(local_path) = url_to_cache_path(cache_dir, resolved.as_str()) {
                            let local_url = format!(
                                "file:///{}",
                                local_path.display().to_string().replace('\\', "/")
                            );
                            replacements.push((raw_url.to_string(), local_url));
                        }
                    }
                }
                search_start = abs_pos + end_pos + 1;
            } else {
                break;
            }
        }
    }

    // Also handle url(...) in inline styles
    let css_prefixes: Vec<(&str, &str)> = vec![
        ("url(\"", "\""),
        ("url('", "'"),
    ];

    for (prefix, suffix) in &css_prefixes {
        let mut search_start = 0;
        while let Some(pos) = result[search_start..].find(*prefix) {
            let abs_pos = search_start + pos + prefix.len();
            if let Some(end_pos) = result[abs_pos..].find(*suffix) {
                let raw_url = &result[abs_pos..abs_pos + end_pos];
                if !raw_url.is_empty()
                    && !raw_url.starts_with("data:")
                    && !raw_url.starts_with("file:")
                {
                    if let Ok(resolved) = base.join(raw_url) {
                        if let Some(local_path) = url_to_cache_path(cache_dir, resolved.as_str()) {
                            let local_url = format!(
                                "file:///{}",
                                local_path.display().to_string().replace('\\', "/")
                            );
                            replacements.push((raw_url.to_string(), local_url));
                        }
                    }
                }
                search_start = abs_pos + end_pos + 1;
            } else {
                break;
            }
        }
    }

    // Apply replacements - replace both "url" and 'url' forms
    for (original, local) in replacements {
        let with_double = format!("\"{}\"", original);
        let with_double_new = format!("\"{}\"", local);
        let with_single = format!("'{}'", original);
        let with_single_new = format!("'{}'", local);
        result = result.replace(&with_double, &with_double_new);
        result = result.replace(&with_single, &with_single_new);
    }

    result
}
