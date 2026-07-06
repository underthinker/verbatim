fn main() {
    // tauri::generate_context! requires the frontendDist path to exist at
    // compile time. Create it so Rust-only builds (CI clippy/test matrix,
    // fresh checkouts) compile before the frontend has been built; release
    // bundling builds ui/dist first, so the real assets are embedded there.
    if let Err(err) = std::fs::create_dir_all("../../ui/dist") {
        println!("cargo::warning=could not create ui/dist: {err}");
    }
    tauri_build::build();
}
