// The CLI uses native filesystem and networking APIs. Keep the implementation out of
// wasm builds while retaining Cargo automatic binary target discovery for the workspace.
#[cfg(not(target_arch = "wasm32"))]
include!("ipfs/native.rs");

// The library supports wasm, but the native executable does not.
#[cfg(target_arch = "wasm32")]
fn main() {}
