use std::{env, fs, path::{Path, PathBuf}};

fn walk_dir(dir: &Path, base: &Path, results: &mut Vec<PathBuf>) {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk_dir(&path, base, results);
            } else if let Ok(rel) = path.strip_prefix(base) {
                results.push(rel.to_path_buf());
            }
        }
    }
}

fn main() {
    tauri_build::build();

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());

    // --- Generate config constants from app.config.json ---
    let config_path = manifest_dir.parent().unwrap().join("app.config.json");
    println!("cargo:rerun-if-changed={}", config_path.display());
    let config_str = fs::read_to_string(&config_path).expect("Failed to read app.config.json");
    let config: serde_json::Value = serde_json::from_str(&config_str).expect("Invalid app.config.json");
    let remote_url = config["remote"]["url"].as_str().unwrap();
    let remote_base_url = config["remote"]["baseUrl"].as_str().unwrap();
    let cache_dir = config["cache"]["directory"].as_str().unwrap();
    let max_redirects = config["cache"]["maxRedirects"].as_u64().unwrap();
    let config_code = format!(
        "pub(crate) const REMOTE_URL:&str=\"{}\";pub(crate) const REMOTE_BASE_URL:&str=\"{}\";pub(crate) const CACHE_DIR:&str=\"{}\";pub(crate) const MAX_REDIRECTS:usize={};",
        remote_url, remote_base_url, cache_dir, max_redirects
    );
    fs::write(out_dir.join("config_generated.rs"), &config_code).expect("Failed to write config_generated.rs");

    // --- Generate bundled cache data ---
    let bundled_cache_dir = manifest_dir.join("bundled-cache");
    let generated_file = out_dir.join("bundled_cache_data.rs");
    if bundled_cache_dir.exists() && bundled_cache_dir.is_dir() {
        let mut files: Vec<PathBuf> = Vec::new();
        walk_dir(&bundled_cache_dir, &bundled_cache_dir, &mut files);
        files.retain(|f| f.to_str() != Some("_manifest.json"));
        let mut code = String::from("pub(crate) const BUNDLED_FILES:&[(&str,&[u8])]=&[");
        for file in &files {
            let file_str = file.to_str().unwrap().replace('\\', "/");
            let abs_path_str = bundled_cache_dir.join(file).to_str().unwrap().replace('\\', "/");
            code.push_str(&format!("(\"{}\",include_bytes!(\"{}\")),", file_str, abs_path_str));
        }
        code.push_str("];");
        fs::write(&generated_file, &code).expect("Failed to write bundled_cache_data.rs");
        println!("cargo:rerun-if-changed=bundled-cache");
        for file in &files {
            println!("cargo:rerun-if-changed={}", bundled_cache_dir.join(file).display());
        }
    } else {
        fs::write(&generated_file, "pub(crate) const BUNDLED_FILES:&[(&str,&[u8])]=&[];").unwrap();
    }
}
