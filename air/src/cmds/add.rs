//! `air add` -- install a reviewed local provider package without networking.

use wasmtime::Result;

pub fn cmd_add(source: &str, package: &str) -> Result<()> {
    crate::provider::add(source, package)
}
