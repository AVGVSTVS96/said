fn main() {
    tauri::Builder::default()
        .run(tauri::generate_context!())
        .expect("could not run the app shell");
}
