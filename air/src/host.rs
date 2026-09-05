//! Host state: WASI ctx and owned handles.
//!
//! The harness owns the ONE shared memory and never depends on guest
//! exports to find it (a lib instance may export no memory at all).
//!
//! There is no socket table here any more, and no UI command queue. Both
//! existed for Core-only host namespaces: `net.*` retired to `wasi:sockets`,
//! `ui.*` to the `ai-direct:host/ui` component interface.

use wasmtime::{Caller, Memory, Result};

use wasmtime_wasi::p1::WasiP1Ctx;

pub struct Host {
    pub wasi: WasiP1Ctx,
    pub shared: Option<Memory>,
    pub term_active: bool,
}

impl Host {
    pub fn new(wasi: WasiP1Ctx) -> Self {
        Host {
            wasi,
            shared: None,
            term_active: false,
        }
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        // A guest trap, Ctrl-C, or failed run must never strand the user's
        // terminal in raw mode or the alternate screen.
        crate::term::restore(self);
    }
}

pub fn shared_mem(caller: &mut Caller<'_, Host>) -> Result<Memory> {
    caller
        .data()
        .shared
        .clone()
        .ok_or_else(|| wasmtime::Error::msg("harness memory not installed"))
}
