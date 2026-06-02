use std::{
    env, fs,
    path::{Path, PathBuf},
};

/// Walk a directory recursively and return all file paths relative to `base`.
fn walk_dir(dir: &Path, base: &Path, results: &mut Vec<PathBuf>) {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk_dir(&path, base, results);
            } else {
                if let Ok(rel) = path.strip_prefix(base) {
                    results.push(rel.to_path_buf());
                }
            }
        }
    }
}

fn main() {
    tauri_build::build();

    // Generate bundled cache module at compile time
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let bundled_cache_dir = manifest_dir.join("bundled-cache");
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let generated_file = out_dir.join("bundled_cache_data.rs");

    if bundled_cache_dir.exists() && bundled_cache_dir.is_dir() {
        let mut files: Vec<PathBuf> = Vec::new();
        walk_dir(&bundled_cache_dir, &bundled_cache_dir, &mut files);

        // Filter out the manifest file itself
        files.retain(|f| f.to_str() != Some("_manifest.json"));

        let mut code = String::new();
        code.push_str("/// Auto-generated bundled cache data. Do not edit.\n");
        code.push_str("pub(crate) const BUNDLED_FILES: &[(&str, &[u8])] = &[\n");

        for file in &files {
            let file_str = file.to_str().unwrap().replace('\\', "/");
            let abs_path = bundled_cache_dir.join(file);
            let abs_path_str = abs_path.to_str().unwrap().replace('\\', "/");
            code.push_str(&format!(
                "    (\"{}\", include_bytes!(\"{}\")),\n",
                file_str, abs_path_str
            ));
        }

        code.push_str("];\n");

        fs::write(&generated_file, &code).expect("Failed to write bundled_cache_data.rs");

        // Tell cargo to re-run if bundled-cache changes
        println!("cargo:rerun-if-changed=bundled-cache");
        for file in &files {
            let abs_path = bundled_cache_dir.join(file);
            println!("cargo:rerun-if-changed={}", abs_path.display());
        }
    } else {
        // No bundled cache - generate empty array
        let code = "/// No bundled cache available.\npub(crate) const BUNDLED_FILES: &[(&str, &[u8])] = &[];\n";
        fs::write(&generated_file, code).expect("Failed to write bundled_cache_data.rs");
    }
}
