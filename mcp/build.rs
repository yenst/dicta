fn build_macos_native() {
    cc::Build::new()
        .file("native/FrameExtractor.m")
        .flag("-fobjc-arc")
        .flag("-mmacosx-version-min=15.0")
        .compile("dicta_mcp_frames");

    println!("cargo:rustc-link-lib=framework=AVFoundation");
    println!("cargo:rustc-link-lib=framework=AppKit");
    println!("cargo:rustc-link-lib=framework=CoreMedia");
    println!("cargo:rustc-link-lib=framework=Foundation");
    println!("cargo:rerun-if-changed=native/FrameExtractor.m");
}

fn main() {
    println!("cargo:rerun-if-env-changed=CARGO_CFG_TARGET_OS");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        build_macos_native();
    }
}
