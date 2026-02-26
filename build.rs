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
    println!("cargo:rerun-if-changed=frontend/");
}
