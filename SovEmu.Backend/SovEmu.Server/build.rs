// `sqlx::migrate!` embeds `migrations/` at compile time, and cargo does not watch that folder by
// itself: without this, a new migration can be left out of an incremental build.
//
// SovEmu's own ScreenScraper developer credentials go into release builds from two variables
// set only while CI builds the image (`SOVEMU_BUILD_SCREENSCRAPER_DEVID`, `_DEVPASSWORD`, from
// GitHub secrets through a BuildKit secret mount), never from the repository. They are written
// scrambled (XOR with `KEY`, which `main.rs` undoes), only so they do not show in `strings`: any
// public binary's credentials can be read by someone determined, as every frontend's can.
// Unset (a local build, a fork's CI), none are built in.

/// Keep in step with `BUILTIN_KEY` in `src/main.rs`.
const KEY: &[u8] = b"SovEmu/ScreenScraper";

fn scrambled(var: &str) -> Vec<u8> {
    println!("cargo:rerun-if-env-changed={var}");
    let value = std::env::var(var).unwrap_or_default();
    value.trim().bytes().zip(KEY.iter().cycle()).map(|(b, k)| b ^ k).collect()
}

fn main() {
    println!("cargo:rerun-if-changed=migrations");
    let id = scrambled("SOVEMU_BUILD_SCREENSCRAPER_DEVID");
    let password = scrambled("SOVEMU_BUILD_SCREENSCRAPER_DEVPASSWORD");
    let out =
        std::path::Path::new(&std::env::var("OUT_DIR").unwrap()).join("builtin_screenscraper.rs");
    std::fs::write(
        out,
        format!(
            "/// SovEmu's ScreenScraper developer id and password, scrambled (`build.rs`); empty \
             when the build had none.\nconst BUILTIN_DEVID: &[u8] = &{id:?};\n\
             const BUILTIN_DEVPASSWORD: &[u8] = &{password:?};\n"
        ),
    )
    .unwrap();
}
