//! Finished-lib demo: the crates.io `sha2` crate exposed as a linkable
//! WASM module. The host loads `sha256.wasm`, allocates buffers inside the
//! LIB's own memory, calls in, and copies the digest back to the app.
//!
//! Build: `cargo build --release --target wasm32-wasip1`
//! Artifact: `target/wasm32-wasip1/release/sha256.wasm` -> copy to `lib/`.

use sha2::{Digest, Sha256};

/// Bump-allocate `n` bytes inside lib memory. Leaked on purpose: the host
/// owns buffer lifetimes, the lib just needs stable addresses per call.
#[no_mangle]
pub extern "C" fn sha256_alloc(n: u32) -> *mut u8 {
    let mut v = vec![0u8; n as usize];
    let p = v.as_mut_ptr();
    std::mem::forget(v);
    p
}

/// Hex SHA-256 of `[input_ptr, input_ptr+input_len)` into 64 bytes at
/// `out_ptr`. All pointers are in THIS module's memory. Returns 0 on success.
#[no_mangle]
pub extern "C" fn sha256_hex(input_ptr: *const u8, input_len: u32, out_ptr: *mut u8) -> i32 {
    if input_ptr.is_null() || out_ptr.is_null() {
        return -1;
    }
    // SAFETY: host guarantees valid ranges (it allocated them itself).
    let input = unsafe { std::slice::from_raw_parts(input_ptr, input_len as usize) };
    let out = unsafe { std::slice::from_raw_parts_mut(out_ptr, 64) };
    let digest = Sha256::digest(input);
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for (i, b) in digest.iter().enumerate() {
        out[2 * i] = HEX[(b >> 4) as usize];
        out[2 * i + 1] = HEX[(b & 15) as usize];
    }
    0
}
