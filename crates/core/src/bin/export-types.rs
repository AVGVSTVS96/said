fn main() {
    let output = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../app/src/bindings");
    voice_core::bindings::export(output).unwrap();
}
