#[cfg(target_family = "wasm")]
include!("main.rs");

#[cfg(not(target_family = "wasm"))]
fn main() {
    eprintln!("Build extensions-web for wasm32-unknown-unknown using its build.sh script.");
}
