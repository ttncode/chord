// Puts the app icon into chord.exe. Only the MSVC toolchain ships a resource compiler (rc.exe);
// GNU builds (no windres installed) skip it and get the default icon.
fn main() {
    println!("cargo:rerun-if-changed=assets/chord.rc");
    println!("cargo:rerun-if-changed=assets/chord.ico");
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        embed_resource::compile("assets/chord.rc", embed_resource::NONE).manifest_optional().unwrap();
    }
}
