//! Hands the compiler's path to the tests: they build a stand-in for the
//! tools with this same compiler (`testing::stand_in`), on every platform.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    if let Ok(rustc) = std::env::var("RUSTC") {
        println!("cargo:rustc-env=OSPREY_RUSTC={rustc}");
    }
}
