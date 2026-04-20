fn main() {
    // Declare the custom cfg flag so rustc's check-cfg lint is satisfied.
    println!("cargo::rustc-check-cfg=cfg(svtav1_available)");
    println!("cargo:rerun-if-env-changed=SVTAV1_LIB_DIR");
    println!("cargo:rerun-if-changed=wrapper.h");

    let out_dir = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());

    // Locate SVT-AV1 headers.  Allow override via SVTAV1_INCLUDE_DIR.
    let default_include_dirs: Vec<String> = vec![
        "/usr/local/include/svt-av1".to_string(),
        "/usr/include/svt-av1".to_string(),
    ];
    let include_dirs: Vec<String> = if let Ok(d) = std::env::var("SVTAV1_INCLUDE_DIR") {
        vec![d]
    } else {
        default_include_dirs
    };

    let header_found = include_dirs
        .iter()
        .any(|p| std::path::Path::new(p).join("EbSvtAv1Enc.h").exists());

    if header_found {
        // Only emit link directives when the library is actually present.
        // This prevents linker failures on systems without libsvtav1.
        println!("cargo:rustc-link-lib=SvtAv1Enc");
        if let Ok(lib_dir) = std::env::var("SVTAV1_LIB_DIR") {
            println!("cargo:rustc-link-search=native={lib_dir}");
        } else {
            println!("cargo:rustc-link-search=native=/usr/local/lib");
        }

        let bindings = bindgen::Builder::default()
            .header("wrapper.h")
            .clang_args(include_dirs.iter().map(|p| format!("-I{p}")))
            .allowlist_type("EbSvtAv1EncConfiguration")
            .allowlist_type("EbComponentType")
            .allowlist_type("EbSvtIOFormat")
            .allowlist_type("EbBufferHeaderType")
            .allowlist_function("svt_av1_enc_init_handle")
            .allowlist_function("svt_av1_enc_set_parameter")
            .allowlist_function("svt_av1_enc_init")
            .allowlist_function("svt_av1_enc_send_picture")
            .allowlist_function("svt_av1_enc_get_packet")
            .allowlist_function("svt_av1_enc_release_out_buffer")
            .allowlist_function("svt_av1_enc_deinit")
            .allowlist_function("svt_av1_enc_deinit_handle")
            .allowlist_var("EB_.*")
            .generate()
            .expect("bindgen failed to generate SVT-AV1 bindings");

        bindings
            .write_to_file(out_dir.join("svt_av1_bindings.rs"))
            .expect("failed to write SVT-AV1 bindings");

        println!("cargo:rustc-cfg=svtav1_available");
    } else {
        // Write an empty bindings file so include! in svt_av1.rs doesn't fail.
        std::fs::write(out_dir.join("svt_av1_bindings.rs"), b"").unwrap();
        // svtav1_available is NOT set — crate builds as no-op stub.
    }
}
