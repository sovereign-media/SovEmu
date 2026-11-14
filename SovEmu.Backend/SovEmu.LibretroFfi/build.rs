//! Generates the libretro bindings from `include/libretro.h` and, for frontends, compiles the
//! C shim that turns libretro's variadic log callback into a plain one Rust can implement.

use std::env;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=include/libretro.h");
    println!("cargo:rerun-if-changed=src/log_shim.c");

    if env::var_os("CARGO_FEATURE_FRONTEND").is_some() {
        cc::Build::new()
            .file("src/log_shim.c")
            .include("include")
            .warnings(true)
            .compile("sovemu_log_shim");
    }

    let bindings = bindgen::Builder::default()
        .header("include/libretro.h")
        // Only the types and constants: SovEmu loads every core at runtime, so it never links
        // against the `retro_*` functions the header declares.
        .allowlist_type("retro_.*")
        .allowlist_var("RETRO_.*")
        // C enums from a core may hold any value; a Rust enum would make that undefined behaviour.
        .default_enum_style(bindgen::EnumVariation::ModuleConsts)
        // The header's Doxygen comments contain code blocks rustdoc would try to run as doctests.
        .generate_comments(false)
        .derive_default(true)
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
        .generate()
        .expect("bindgen failed to generate the libretro bindings");

    let out = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR is set by cargo"));
    bindings.write_to_file(out.join("bindings.rs")).expect("failed to write bindings.rs");
}
