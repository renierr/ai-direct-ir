//! Manifest: what an app is made of. A new project = a new TOML file.

use serde::Deserialize;

#[derive(Deserialize, Clone, Copy)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Server,
    Command,
}

/// The host implementation an app targets. Native remains the default so
/// existing manifests continue to run under Wasmtime.
#[derive(Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Target {
    Native,
    Browser,
    Gui,
    /// WASM Component + WASI 0.2. A separate linking domain from the Core
    /// targets above, so it shares the manifest and nothing else.
    Component,
}

fn default_target() -> Target {
    Target::Native
}

#[derive(Deserialize)]
pub struct Lib {
    pub path: String,
    #[serde(rename = "as")]
    pub namespace: String,
}

#[derive(Deserialize, Clone)]
pub struct BridgeCall {
    #[serde(rename = "as")]
    pub name: String,
    pub func: String,
    pub in_ptr: usize,
    pub in_len: usize,
    pub out_ptr: usize,
    pub out_len: u32,
    #[serde(default = "default_max_in")]
    pub max_in: u32,
}

fn default_max_in() -> u32 {
    1 << 20
}

#[derive(Deserialize)]
pub struct Bridge {
    pub path: String,
    #[serde(rename = "as")]
    pub namespace: String,
    pub alloc: String,
    pub calls: Vec<BridgeCall>,
}

#[derive(Deserialize)]
pub struct App {
    /// Optional WAT source assembled by `host-rs build` into `path`.
    pub source: Option<String>,
    pub path: String,
    pub run: String,
}

#[derive(Deserialize)]
pub struct Manifest {
    #[serde(default = "default_target")]
    pub target: Target,
    pub mode: Mode,
    pub port: Option<u16>,
    pub root: Option<String>,
    pub guest: Option<String>,
    pub memory_pages: Option<u32>,
    #[serde(default)]
    pub workers: Option<usize>,
    #[serde(default)]
    pub libs: Vec<Lib>,
    #[serde(default)]
    pub bridges: Vec<Bridge>,
    pub app: App,
}

impl Manifest {
    /// Worker instances for server mode (host-owned accept loop).
    /// 1/absent = legacy: the app's own `run` owns listen+accept.
    pub fn worker_count(&self) -> usize {
        match self.mode {
            Mode::Server => self.workers.unwrap_or(1).max(1),
            Mode::Command => 1,
        }
    }
}

pub fn load(path: &str) -> wasmtime::Result<Manifest> {
    Ok(toml::from_str(&std::fs::read_to_string(path)?)?)
}
