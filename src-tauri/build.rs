fn build_macos_native() {
    cc::Build::new()
        .files(["native/Capture.m", "native/Speech.m", "native/Media.m"])
        .flag("-fobjc-arc")
        .flag("-fblocks")
        .flag("-mmacosx-version-min=15.0")
        .compile("dicta_recorder");

    println!("cargo:rustc-link-lib=framework=ScreenCaptureKit");
    println!("cargo:rustc-link-lib=framework=AVFoundation");
    println!("cargo:rustc-link-lib=framework=Speech");
    println!("cargo:rustc-link-lib=framework=CoreMedia");
    println!("cargo:rustc-link-lib=framework=CoreVideo");
    println!("cargo:rustc-link-lib=framework=Foundation");
    println!("cargo:rustc-link-lib=framework=AppKit");
    println!("cargo:rerun-if-changed=native/DictaNative.h");
    println!("cargo:rerun-if-changed=native/Capture.m");
    println!("cargo:rerun-if-changed=native/Speech.m");
    println!("cargo:rerun-if-changed=native/Media.m");
}

fn main() {
    println!("cargo:rerun-if-env-changed=CARGO_CFG_TARGET_OS");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        build_macos_native();
    }

    tauri_build::build()
}
