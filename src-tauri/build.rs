use std::{
    env, fs,
    path::{Path, PathBuf},
};

fn walk_dir(directory: &Path, base: &Path, results: &mut Vec<PathBuf>) {
    if let Ok(entries) = fs::read_dir(directory) {
        for entry in entries.flatten() {
            let path: PathBuf = entry.path();
            if path.is_dir() {
                walk_dir(&path, base, results);
            } else if let Ok(relative) = path.strip_prefix(base) {
                results.push(relative.to_path_buf());
            }
        }
    }
}

fn main() {
    tauri_build::build();

    let manifest_dir: PathBuf = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let out_dir: PathBuf = PathBuf::from(env::var("OUT_DIR").unwrap());

    let config_path: PathBuf = manifest_dir.parent().unwrap().join("app.config.json");
    println!("cargo:rerun-if-changed={}", config_path.display());
    let config_content: String =
        fs::read_to_string(&config_path).expect("Failed to read app.config.json");
    let config: serde_json::Value =
        serde_json::from_str(&config_content).expect("Invalid app.config.json");
    let remote_url: &str = config["remote"]["url"].as_str().unwrap();
    let cache_directory: &str = config["cache"]["directory"].as_str().unwrap();
    let max_redirects: u64 = config["cache"]["maxRedirects"].as_u64().unwrap();
    let config_code: String = format!(
        "pub(crate) const REMOTE_URL:&str=\"{}\";pub(crate) const CACHE_DIR:&str=\"{}\";pub(crate) const MAX_REDIRECTS:usize={};",
        remote_url, cache_directory, max_redirects
    );
    fs::write(out_dir.join("config_generated.rs"), &config_code)
        .expect("Failed to write config_generated.rs");

    let bundled_cache_dir: PathBuf = manifest_dir.join("bundled-cache");
    let generated_file: PathBuf = out_dir.join("bundled_cache_data.rs");
    if bundled_cache_dir.exists() && bundled_cache_dir.is_dir() {
        let mut files: Vec<PathBuf> = Vec::new();
        walk_dir(&bundled_cache_dir, &bundled_cache_dir, &mut files);
        files.retain(|file_path: &PathBuf| file_path.to_str() != Some("_manifest.json"));
        let mut code: String = String::from("pub(crate) const BUNDLED_FILES:&[(&str,&[u8])]=&[");
        for file in &files {
            let relative_str: String = file.to_str().unwrap().replace('\\', "/");
            let absolute_str: String = bundled_cache_dir
                .join(file)
                .to_str()
                .unwrap()
                .replace('\\', "/");
            code.push_str(&format!(
                "(\"{relative_str}\",include_bytes!(\"{absolute_str}\")),"
            ));
        }
        code.push_str("];");
        fs::write(&generated_file, &code).expect("Failed to write bundled_cache_data.rs");
        println!("cargo:rerun-if-changed=bundled-cache");
        for file in &files {
            println!(
                "cargo:rerun-if-changed={}",
                bundled_cache_dir.join(file).display()
            );
        }
    } else {
        fs::write(
            &generated_file,
            "pub(crate) const BUNDLED_FILES:&[(&str,&[u8])]=&[];",
        )
        .unwrap();
    }
}
