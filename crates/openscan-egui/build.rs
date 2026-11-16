// Reserve room for the post-link ad-hoc or Developer ID signature.
fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        println!("cargo:rustc-link-arg-bin=openscan=-Wl,-headerpad,0x1000");
    }
}
