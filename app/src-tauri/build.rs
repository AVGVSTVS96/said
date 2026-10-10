fn main() {
    println!("cargo:rerun-if-changed=../../product.toml");
    println!("cargo:rerun-if-changed=tauri.base.json");
    let product: toml::Table = std::fs::read_to_string("../../product.toml")
        .unwrap()
        .parse()
        .unwrap();
    let name = product["name"].as_str().unwrap();
    let identifier = product["bundle_id"].as_str().unwrap();
    let storage_key = product["storage_key"].as_str().unwrap();
    let mut config: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string("tauri.base.json").unwrap()).unwrap();
    config["productName"] = name.into();
    config["identifier"] = identifier.into();
    let serialized = serde_json::to_string_pretty(&config).unwrap() + "\n";
    if std::fs::read_to_string("tauri.conf.json").ok().as_deref() != Some(&serialized) {
        std::fs::write("tauri.conf.json", serialized).unwrap();
    }
    println!("cargo:rustc-env=PRODUCT_NAME={name}");
    println!("cargo:rustc-env=PRODUCT_BUNDLE_ID={identifier}");
    println!("cargo:rustc-env=PRODUCT_STORAGE_KEY={storage_key}");
    tauri_build::build();
}
