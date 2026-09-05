//! Manifest: what an app is made of. A new project = a new TOML file.

use serde::Deserialize;

/// How the host enters an application: once, or once per drawn frame.
///
/// `server` retired with the `net.*` syscalls -- a component owns its own
/// accept loop through `wasi:sockets`, so there is nothing left for a
/// host-owned one to do. A manifest that still says `mode = "server"` fails to
/// parse, which is the right way to find out.
#[derive(Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Command,
    /// Open a native window and call the entry point every frame. This is a
    /// host-loop choice, not a linking domain, so it is a `mode` rather than a
    /// `target`: a GUI app is an ordinary component that imports
    /// `ai-direct:host/ui`.
    Gui,
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

/// A directory granted to the application. WASI gives a component no ambient
/// filesystem at all: it reaches exactly the directories listed here, under the
/// names given here, with the permissions given here. Read-only unless the
/// manifest says otherwise, because writing is the exception.
#[derive(Deserialize)]
pub struct Dir {
    pub path: String,
    /// The name the guest sees. Defaults to `path`.
    pub guest: Option<String>,
    #[serde(default)]
    pub write: bool,
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
    /// Only `air serve` reads this now; the retired server mode used it too.
    pub port: Option<u16>,
    pub root: Option<String>,
    pub guest: Option<String>,
    pub memory_pages: Option<u32>,
    #[serde(default)]
    pub libs: Vec<Lib>,
    #[serde(default)]
    pub bridges: Vec<Bridge>,
    #[serde(default)]
    pub providers: Vec<Provider>,
    #[serde(default)]
    pub dirs: Vec<Dir>,
    /// `network = true` lets the app open sockets through `wasi:sockets`.
    /// Nothing is reachable unless it asks, the same rule the directory
    /// grants follow; `air run --net` is the shell-side equivalent.
    #[serde(default)]
    pub network: bool,
    pub app: App,
}

impl Default for Target {
    fn default() -> Self {
        Target::Native
    }
}

/// Decide what a project is, preferring evidence over declaration: the WAT
/// source, then the built artifact, then whatever the manifest said.
///
/// Source first, because the artifact is downstream of it: `air run` rebuilds
/// a stale `.wasm` before entering it, so an artifact left over from what the
/// project used to be must not decide what it is now.
fn resolve_target(manifest: &Manifest, base: &std::path::Path) -> wasmtime::Result<Target> {
    let from_source = manifest
        .app
        .source
        .as_ref()
        .and_then(|source| std::fs::read_to_string(crate::link::join(base, source)).ok())
        .map(|text| is_component_source(&text));
    let is_component = match from_source {
        Some(found) => Some(found),
        None => {
            let mut preamble = [0u8; 8];
            match std::fs::File::open(crate::link::join(base, &manifest.app.path)) {
                Ok(mut file) => {
                    use std::io::Read;
                    let read = file.read(&mut preamble).unwrap_or(0);
                    Some(is_component_binary(&preamble[..read]))
                }
                Err(_) => None,
            }
        }
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
        // A Core artifact can be hosted two ways, so `native` is the default
        // and `browser` stays an explicit choice.
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
    // A frame loop needs `ai-direct:host/ui`, which is a WIT interface: there
    // is no Core host for it any more. Saying so here keeps the failure at the
    // manifest rather than at an unresolved import.
    if manifest.mode == Mode::Gui && manifest.target != Target::Component {
        return Err(wasmtime::Error::msg(format!(
            "`mode = \"gui\"` needs a component; `{}` is a Core WASM module",
            manifest.app.path
        )));
    }
    Ok(manifest)
}
