fn main() {
    #[cfg(target_os = "macos")]
    {
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
}
