//! Links the system libvpx and libopus (found with pkg-config) and generates bindings for the
//! parts of their APIs SovEmu uses. On Debian: `libvpx-dev libopus-dev pkg-config`; on macOS:
//! `brew install libvpx opus pkgconf`.

use std::env;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=wrapper.h");

    let mut include_paths = Vec::new();
    for lib in ["vpx", "opus"] {
        let found = pkg_config::Config::new()
            .probe(lib)
            .unwrap_or_else(|e| panic!("pkg-config could not find {lib}: {e}"));
        include_paths.extend(found.include_paths);
    }

    let mut builder = bindgen::Builder::default()
        .header("wrapper.h")
        .allowlist_function("vpx_.*")
        .allowlist_function("opus_encode")
        .allowlist_function("opus_encoder_.*")
        .allowlist_function("opus_decode")
        .allowlist_function("opus_decoder_.*")
        .allowlist_function("opus_strerror")
        .allowlist_function("opus_packet_get_nb_samples")
        .allowlist_var("VPX_.*")
        .allowlist_var("VP8.*")
        .allowlist_var("OPUS_.*")
        .allowlist_var("vpx_codec_vp8_cx_algo")
        .allowlist_var("vpx_codec_vp8_dx_algo")
        .allowlist_type("vp8e_enc_control_id")
        .default_enum_style(bindgen::EnumVariation::ModuleConsts)
        .generate_comments(false)
        .derive_default(true)
        .layout_tests(false)
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()));
    for path in &include_paths {
        builder = builder.clang_arg(format!("-I{}", path.display()));
        // opus.pc points at .../include/opus; vpx.pc at .../include.
    }

    let out = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR is set by cargo"));
    builder
        .generate()
        .expect("bindgen failed to generate libvpx/libopus bindings")
        .write_to_file(out.join("bindings.rs"))
        .expect("failed to write bindings.rs");
}
