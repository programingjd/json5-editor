//! Export exact-version license evidence for the host-side notice generator.
use std::{collections::BTreeMap, error::Error, fs, io::Read, path::Path};
use serde_json::{Value, json};

fn walk(root: &Path, directory: &Path, documents: &mut Vec<Value>, copyrights: &mut BTreeMap<String, Vec<String>>) -> Result<(), Box<dyn Error>> {
    let mut entries = fs::read_dir(directory)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if entry.file_type()?.is_dir() {
            if !matches!(name.as_str(), "target" | ".git" | "node_modules") {
                walk(root, &path, documents, copyrights)?;
            }
            continue;
        }
        if !entry.file_type()?.is_file() { continue; }
        let relative = path.strip_prefix(root)?.to_string_lossy().replace('\\', "/");
        let lower = name.to_ascii_lowercase();
        let legal = path.strip_prefix(root)?.components().any(|component| matches!(component.as_os_str().to_str(), Some(".licenses" | "licenses" | "license")))
            || ["license", "licence", "copying", "copyright", "notice", "unlicense"]
            .iter().any(|prefix| lower.starts_with(prefix));
        if legal {
            let bytes = fs::read(&path)?;
            if let Ok(content) = String::from_utf8(bytes) {
                documents.push(json!({ "file": relative, "text": content }));
            }
        } else if matches!(path.extension().and_then(|extension| extension.to_str()), Some("rs" | "c" | "h" | "cpp" | "cc" | "S" | "js" | "md")) {
            let mut prefix = Vec::new();
            fs::File::open(&path)?.take(8192).read_to_end(&mut prefix)?;
            let content = String::from_utf8_lossy(&prefix);
            let notices: Vec<_> = content.lines().filter(|line| {
                let line = line.trim_start_matches([' ', '\t', '/', '*', '#', '<', '!', '-']);
                let lower = line.to_lowercase();
                lower.starts_with("copyright") || lower.starts_with('©') || lower.starts_with("portions copyright")
            }).map(str::to_owned).collect();
            if !notices.is_empty() { copyrights.insert(relative, notices); }
        }
    }
    Ok(())
}

pub(super) fn collect(package: &Value) -> Result<Value, Box<dyn Error>> {
    let manifest = Path::new(package["manifest_path"].as_str().ok_or("Package manifest is missing")?);
    let root = manifest.parent().ok_or("Package directory is missing")?;
    let mut documents = Vec::new();
    let mut copyrights = BTreeMap::new();
    walk(root, root, &mut documents, &mut copyrights)?;
    if let Some(file) = package["license_file"].as_str() {
        let path = root.join(file);
        if path.is_file() && !documents.iter().any(|document| document["file"] == file) {
            documents.push(json!({ "file": file, "text": fs::read_to_string(path)? }));
        }
    }
    let vcs = fs::read_to_string(root.join(".cargo_vcs_info.json")).ok()
        .and_then(|content| serde_json::from_str::<Value>(&content).ok());
    Ok(json!({
        "name": package["name"], "version": package["version"],
        "license": package["license"], "repository": package["repository"],
        "cargo_source": package["source"],
        "authors": package["authors"], "vcs": vcs,
        "documents": documents, "source_copyrights": copyrights
    }))
}
