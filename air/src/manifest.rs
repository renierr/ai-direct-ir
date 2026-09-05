//! Manifest: what an app is made of. A new project = a new TOML file.

use serde::Deserialize;

#[derive(Deserialize, Clone, Copy)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Server,
    Command,
}

/// The host implementation an app targets. It is normally inferred: a WASM
/// artifact's own preamble says whether it is a component or a Core module, so
/// a manifest only has to state a target to pick between the Core hosts, or to
/// declare intent and get an error when the file disagrees.
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

/// True when `bytes` begins with the component preamble. Core modules declare
/// layer 0 (`01 00`); components declare layer 1 (`0d 00`).
pub fn is_component_binary(bytes: &[u8]) -> bool {
    bytes.len() >= 8 && bytes[..4] == *b"\0asm" && bytes[4..6] == [0x0d, 0x00]
}

/// True when WAT source opens a `(component ...)` rather than a `(module ...)`.
/// Leading comments are skipped; nothing else can precede the outermost form.
pub fn is_component_source(text: &str) -> bool {
    let bytes = text.as_bytes();
    let mut i = 0usize;
    let mut block = 0usize;
    while i < bytes.len() {
        if block > 0 {
            if bytes[i] == b'(' && bytes.get(i + 1) == Some(&b';') {
                block += 1;
                i += 2;
            } else if bytes[i] == b';' && bytes.get(i + 1) == Some(&b')') {
                block -= 1;
                i += 2;
            } else {
                i += 1;
            }
            continue;
        }
        match bytes[i] {
            b';' if bytes.get(i + 1) == Some(&b';') => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            b'(' if bytes.get(i + 1) == Some(&b';') => {
                block = 1;
                i += 2;
            }
            b'(' => return text[i + 1..].trim_start().starts_with("component"),
            _ => i += 1,
        }
    }
    false
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

/// A component whose exports satisfy the application component's imports.
/// `air` instantiates it and forwards its exported functions at link time,
/// so no build-time composition tool is involved. The trade is what ships: the
/// bundle carries the provider alongside the app rather than one fused
/// component, and resource handles do not cross the boundary.
#[derive(Deserialize)]
pub struct Provider {
    /// Optional WAT source, assembled into `path` like an application's.
    /// Declaring it is what keeps a provider artifact from drifting from the
    /// source beside it.
    pub source: Option<String>,
    pub path: String,
}

#[derive(Deserialize)]
pub struct App {
    /// Optional WAT source assembled by `air build` into `path`.
    pub source: Option<String>,
    pub path: String,
    pub run: String,
}

#[derive(Deserialize)]
pub struct Manifest {
    /// What the manifest asked for, if anything. `target` below is what the
    /// project actually is.
    #[serde(default, rename = "target")]
    pub declared_target: Option<Target>,
    /// The effective target, resolved from the artifact or its source.
    #[serde(skip)]
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
    #[serde(default)]
    pub providers: Vec<Provider>,
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

impl Default for Target {
    fn default() -> Self {
        Target::Native
    }
}

/// Decide what a project is, preferring evidence over declaration: the built
/// artifact, then its WAT source, then whatever the manifest said.
fn resolve_target(manifest: &Manifest, base: &std::path::Path) -> wasmtime::Result<Target> {
    let mut preamble = [0u8; 8];
    let artifact = crate::link::join(base, &manifest.app.path);
    let is_component = match std::fs::File::open(&artifact) {
        Ok(mut file) => {
            use std::io::Read;
            let read = file.read(&mut preamble).unwrap_or(0);
            Some(is_component_binary(&preamble[..read]))
        }
        Err(_) => manifest
            .app
            .source
            .as_ref()
            .and_then(|source| std::fs::read_to_string(crate::link::join(base, source)).ok())
            .map(|text| is_component_source(&text)),
    };
    match (manifest.declared_target, is_component) {
        // Evidence and declaration disagree: say so rather than fail later
        // inside the wrong linker.
        (Some(Target::Component), Some(false)) => Err(wasmtime::Error::msg(format!(
            "manifest declares `target = \"component\"` but `{}` is a Core WASM module",
            manifest.app.path
        ))),
        (Some(declared), Some(true)) if declared != Target::Component => {
            Err(wasmtime::Error::msg(format!(
                "`{}` is a component, but the manifest declares a Core target",
                manifest.app.path
            )))
        }
        (Some(declared), _) => Ok(declared),
        (None, Some(true)) => Ok(Target::Component),
        // A Core artifact can be hosted three ways, so `native` is the default
        // and `browser`/`gui` stay an explicit choice.
        (None, _) => Ok(Target::Native),
    }
}

pub fn load(path: &str) -> wasmtime::Result<Manifest> {
    let mut manifest: Manifest = toml::from_str(&std::fs::read_to_string(path)?)?;
    let base = std::path::Path::new(path)
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .map(std::path::Path::to_path_buf)
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    manifest.target = resolve_target(&manifest, &base)?;
    Ok(manifest)
}
