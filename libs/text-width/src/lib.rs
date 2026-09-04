//! `unicode-width` exposed as a tiny linkable core-WASM library.
//!
//! WAT owns prompt state and drawing policy. This library owns Unicode display
//! cell widths: UTF-8 bytes, Unicode scalar count, and terminal columns differ.
//! Build: `cargo build --release --target wasm32-wasip1`, then copy the output
//! wasm beside this Cargo.toml. The harness bridge copies app bytes in and a
//! little-endian u32 result back out.

use unicode_width::UnicodeWidthStr;

#[unsafe(no_mangle)]
pub extern "C" fn text_width_alloc(n: u32) -> *mut u8 {
    let mut bytes = vec![0u8; n as usize];
    let ptr = bytes.as_mut_ptr();
    std::mem::forget(bytes);
    ptr
}

/// Write the terminal cell width of a UTF-8 byte slice to `out_ptr`.
/// Returns 0 on success, -1 for null/invalid UTF-8 input.
#[unsafe(no_mangle)]
pub extern "C" fn text_width_utf8(input_ptr: *const u8, input_len: u32, out_ptr: *mut u8) -> i32 {
    if input_ptr.is_null() || out_ptr.is_null() {
        return -1;
    }
    // SAFETY: the bridge allocates and bounds-checks the ranges it supplies.
    let input = unsafe { std::slice::from_raw_parts(input_ptr, input_len as usize) };
    let Ok(text) = std::str::from_utf8(input) else {
        return -1;
    };
    // Terminal SGR/CSI sequences consume UTF-8 bytes but zero screen cells.
    // Keep the generic ABI useful to WAT renderers that pass styled labels.
    let mut visible = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' && chars.peek() == Some(&'[') {
            chars.next();
            // ANSI CSI ends at one byte in 0x40..=0x7e (usually `m`).
            while let Some(next) = chars.next() {
                if ('\x40'..='\x7e').contains(&next) {
                    break;
                }
            }
        } else {
            visible.push(c);
        }
    }
    let width = UnicodeWidthStr::width(visible.as_str()) as u32;
    // SAFETY: the bridge allocated exactly four output bytes.
    unsafe { std::ptr::write_unaligned(out_ptr.cast::<u32>(), width) };
    0
}
