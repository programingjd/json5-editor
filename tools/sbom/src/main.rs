use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    fs,
    path::Path,
    process::Command,
};

use cyclonedx_bom::prelude::*;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
mod license_inputs;

fn read_json(path: impl AsRef<Path>) -> Result<Value, Box<dyn Error>> {
    Ok(serde_json::from_slice(&fs::read(path)?)?)
}

fn text<'a>(value: &'a Value, key: &str) -> &'a str {
    value[key].as_str().unwrap_or_default()
}

fn references(repository: &str) -> Value {
    json!([{ "type": "vcs", "url": repository }])
}

fn main() -> Result<(), Box<dyn Error>> {
    let root = std::env::args()
        .nth(1)
        .ok_or("Expected the standalone project directory")?;
    let root = Path::new(&root);
    let output = Command::new("cargo")
        .args([
            "metadata",
            "--locked",
            "--format-version",
            "1",
            "--filter-platform",
            "wasm32-unknown-unknown",
            "--manifest-path",
        ])
        .arg(root.join("Cargo.toml"))
        .output()?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned().into());
    }
    let metadata: Value = serde_json::from_slice(&output.stdout)?;
    let frontend = read_json(root.join("target/build-tools/sbom-input.json"))?;
    let packages = metadata["packages"]
        .as_array()
        .ok_or("Cargo packages are missing")?;
    let nodes: BTreeMap<&str, &Value> = metadata["resolve"]["nodes"]
        .as_array()
        .ok_or("Cargo resolution is missing")?
        .iter()
        .map(|node| (text(node, "id"), node))
        .collect();
    let package_map: BTreeMap<&str, &Value> =
        packages.iter().map(|pkg| (text(pkg, "id"), pkg)).collect();
    let app = packages
        .iter()
        .find(|pkg| text(pkg, "name") == "json5-web-editor")
        .ok_or("Editor package is missing")?;
    let worker = packages
        .iter()
        .find(|pkg| text(pkg, "name") == "gpui-json5-processor")
        .ok_or("Worker package is missing")?;
    let mut pending = vec![text(app, "id"), text(worker, "id")];
    let mut reached = BTreeSet::new();
    let mut edges = BTreeMap::<&str, Vec<&str>>::new();
    while let Some(id) = pending.pop() {
        if !reached.insert(id) {
            continue;
        }
        let node = nodes.get(id).ok_or("A Cargo dependency node is missing")?;
        let dependencies: Vec<&str> = node["deps"]
            .as_array()
            .ok_or("Cargo node dependencies are missing")?
            .iter()
            .filter(|dep| {
                dep["dep_kinds"].as_array().is_some_and(|kinds| {
                    kinds
                        .iter()
                        .any(|kind| kind["kind"].as_str() != Some("dev"))
                })
            })
            .map(|dep| text(dep, "pkg"))
            .collect();
        pending.extend(dependencies.iter().copied());
        edges.insert(id, dependencies);
    }
    let ids: BTreeMap<&str, String> = reached
        .iter()
        .map(|id| {
            let pkg = package_map[id];
            (
                *id,
                format!("cargo:{}@{}", text(pkg, "name"), text(pkg, "version")),
            )
        })
        .collect();
    if ids.values().collect::<BTreeSet<_>>().len() != ids.len() {
        return Err("Cargo package references are ambiguous".into());
    }
    let mut components = Vec::new();
    let mut license_inputs = Vec::new();
    let mut app_component = Value::Null;
    for id in &reached {
        let pkg = package_map[id];
        let name = text(pkg, "name");
        let version = text(pkg, "version");
        if !matches!(name, "json5-web-editor" | "gpui-json5-processor") {
            license_inputs.push(license_inputs::collect(pkg)?);
        }
        let mut component = json!({ "type": if *id == text(app, "id") { "application" } else { "library" },
            "bom-ref": ids[id], "name": name, "version": version, "purl": format!("pkg:cargo/{name}@{version}") });
        if let Some(license) = pkg["license"].as_str() {
            component["licenses"] = json!([{ "expression": license.replace('/', " OR ") }]);
        }
        let checksum_key = format!("{name}@{version}");
        if pkg["source"]
            .as_str()
            .is_some_and(|source| source.starts_with("registry+"))
        {
            if let Some(checksum) = frontend["cargo_checksums"][&checksum_key].as_str() {
                component["hashes"] = json!([{ "alg": "SHA-256", "content": checksum }]);
            }
        }
        let overlay = Path::new(text(pkg, "manifest_path")).starts_with(root.join("crates"));
        if overlay {
            component["externalReferences"] =
                references("https://github.com/programingjd/gpui-kit");
            component["properties"] =
                json!([{ "name": "gpui-json5:source", "value": "editor-owned GPUI Kit subset" }]);
        } else if let Some(repository) = pkg["repository"].as_str() {
            component["externalReferences"] = references(repository);
        }
        if *id == text(app, "id") {
            app_component = component;
        } else {
            components.push(component);
        }
    }
    fs::write(root.join("target/build-tools/license-input.json"), serde_json::to_vec_pretty(&license_inputs)?)?;
    let mut dependencies: Vec<Value> = edges.iter().map(|(id, deps)| json!({ "ref": ids[id], "dependsOn": deps.iter().map(|dep| ids[dep].clone()).collect::<Vec<_>>() })).collect();
    components.extend(
        frontend["components"]
            .as_array()
            .ok_or("Frontend components are missing")?
            .iter()
            .cloned(),
    );
    dependencies.extend(
        frontend["dependencies"]
            .as_array()
            .ok_or("Frontend dependencies are missing")?
            .iter()
            .cloned(),
    );
    let mut asset_refs = Vec::new();
    for (file, license) in [
        ("fonts/Inter.woff2", Some("OFL-1.1")),
        ("fonts/Inter-ital.woff2", Some("OFL-1.1")),
        ("fonts/CommitMono.woff2", Some("OFL-1.1")),
        ("../editor/src/wasm/gpui_json5_editor_web_bg.wasm", None),
        ("../editor/src/worker-wasm/gpui_json5_processor_bg.wasm", None),
    ] {
        let bytes = fs::read(root.join("assets").join(file))?;
        let name = Path::new(file)
            .file_name()
            .ok_or("Asset name is missing")?
            .to_string_lossy();
        let reference = format!("asset:{name}");
        let mut component = json!({ "type": "file", "bom-ref": reference, "name": name,
            "hashes": [{ "alg": "SHA-256", "content": format!("{:x}", Sha256::digest(bytes)) }] });
        if let Some(license) = license {
            component["licenses"] = json!([{ "expression": license }]);
        }
        components.push(component);
        dependencies.push(json!({ "ref": reference, "dependsOn": [] }));
        asset_refs.push(reference);
    }
    let icon_manifest = read_json(root.join("assets/icons.json"))?;
    let jetbrains = &icon_manifest["sources"]["jetbrains"];
    components.push(json!({
        "type": "library", "bom-ref": "assets:jetbrains-icons", "name": jetbrains["name"],
        "licenses": [{ "expression": jetbrains["license"] }],
        "externalReferences": [
            { "type": "website", "url": jetbrains["catalogue"] },
            { "type": "vcs", "url": jetbrains["repository"] }
        ],
        "properties": [
            { "name": "source:retrieved", "value": jetbrains["retrieved"] },
            { "name": "source:icon-manifest", "value": "assets/icons.json" }
        ]
    }));
    dependencies.push(json!({ "ref": "assets:jetbrains-icons", "dependsOn": [] }));
    asset_refs.push("assets:jetbrains-icons".to_owned());
    if let Some(icons) = icon_manifest["icons"].as_array() {
        for icon in icons.iter().filter(|icon| icon["provider"] == "zed") {
            let name = text(icon, "file");
            let reference = format!("asset:{name}");
            components.push(json!({
                "type": "file", "bom-ref": reference, "name": name,
                "hashes": [{ "alg": "SHA-256", "content": icon["sha256"] }],
                "externalReferences": [
                    { "type": "website", "url": icon["source"] },
                    { "type": "license", "url": icon["usage_terms"] }
                ]
            }));
            dependencies.push(json!({ "ref": reference, "dependsOn": [] }));
            asset_refs.push(reference);
        }
    }
    let app_edges = dependencies
        .iter_mut()
        .find(|edge| edge["ref"].as_str() == Some(&ids[text(app, "id")]))
        .ok_or("Application dependencies are missing")?;
    let app_deps = app_edges["dependsOn"]
        .as_array_mut()
        .ok_or("Application edges are invalid")?;
    app_deps.push(json!(ids[text(worker, "id")]));
    app_deps.extend(
        frontend["roots"]
            .as_array()
            .ok_or("Frontend roots are missing")?
            .iter()
            .cloned(),
    );
    app_deps.extend(asset_refs.into_iter().map(Value::String));
    let document = json!({ "bomFormat": "CycloneDX", "specVersion": "1.5", "version": 1,
        "metadata": { "component": app_component, "tools": [{ "name": "cyclonedx-bom", "version": "0.8.1" }] },
        "components": components, "dependencies": dependencies });
    let bom = Bom::parse_from_json_v1_5(&serde_json::to_vec(&document)?[..])?;
    let validation = bom.validate();
    if !validation.passed() {
        return Err(format!("SBOM validation failed: {validation:?}").into());
    }
    let mut serialized = Vec::new();
    bom.output_as_json_v1_5(&mut serialized)?;
    serialized.push(b'\n');
    let destination = root.join("assets/sbom.json");
    fs::write(&destination, serialized)?;
    println!(
        "Generated {} with {} components",
        destination.display(),
        components.len()
    );
    Ok(())
}
