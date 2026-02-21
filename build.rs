fn main() {
    // Install dependencies if needed
    std::process::Command::new("npm")
        .arg("install")
        .current_dir("frontend")
        .status()
        .expect("npm install failed");
    // Build SvelteKit static assets
    std::process::Command::new("npm")
        .arg("run")
        .arg("build")
        .current_dir("frontend")
        .status()
        .expect("npm run build failed");
    // Copy built assets to static/pwa for embedding
    std::fs::create_dir_all("static/pwa").unwrap();
    std::process::Command::new("cp")
        .arg("-r")
        .arg("frontend/build/.")
        .arg("static/pwa/")
        .status()
        .expect("Copy failed");
    println!("cargo:rerun-if-changed=frontend/");
    println!("cargo:rerun-if-changed=static/pwa/");
}
