//! Native GUI target: stable WAT ABI and its egui runtime implementation.

pub mod abi;
mod runtime;

pub use runtime::run;
