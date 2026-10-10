#[path = "src/types.rs"]
#[allow(dead_code)]
mod types;

#[path = "src/bindings.rs"]
mod bindings;

fn main() {
    println!("cargo:rerun-if-changed=src/types.rs");
    println!("cargo:rerun-if-changed=src/bindings.rs");
    let output = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../app/src/bindings");
    bindings::export(output).unwrap();
}
