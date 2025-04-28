fn main() {
    // Tell Cargo to rebuild if this build script changes
    println!("cargo:rerun-if-changed=build.rs");
    
    // No special build steps needed
}