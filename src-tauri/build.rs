fn main() {
    // Re-embed the UI whenever any file in ../ui changes (otherwise Cargo can keep serving
    // an old copy of index.html, app.js or style.css inside the binary).
    println!("cargo:rerun-if-changed=../ui");
    tauri_build::build()
}
