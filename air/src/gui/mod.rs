//! Native GUI mode: the `ai-direct:host/ui` commands and their egui runtime.
//!
//! A GUI app is an ordinary WASI 0.2 component. It differs from a command only
//! in how the host enters it -- once per drawn frame instead of once -- which
//! is why `gui` is a `mode` in the manifest and not a `target`.

mod runtime;

pub use runtime::run;

/// One thing the guest asked to be drawn, in the order it asked.
///
/// The guest describes a whole frame and returns; the runtime replays the
/// list into egui afterwards. Recording rather than rendering is what keeps
/// the guest out of the widget library: it never holds an egui handle, and a
/// trap mid-frame loses the frame rather than the window.
pub enum UiCommand {
    Label(String),
    Button(String),
}
