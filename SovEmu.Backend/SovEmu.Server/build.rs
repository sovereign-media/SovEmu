// `sqlx::migrate!` embeds `migrations/` at compile time, and cargo does not watch that folder by
// itself: without this, a new migration can be left out of an incremental build.
fn main() {
    println!("cargo:rerun-if-changed=migrations");
}
