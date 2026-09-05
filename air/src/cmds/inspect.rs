//! `air inspect` -- report a foreign artifact's imports and exports.

use wasmtime::{Engine, ExternType, Result};

use super::func_sig;

/// Show what a (possibly foreign) module needs and offers — the starting
/// point for wiring any language's .wasm output into a manifest.
pub fn cmd_inspect(engine: &Engine, path: &str) -> Result<()> {
    let m = wasmtime::Module::from_file(engine, path)?;
    println!("module: {path}");
    println!("imports:");
    let mut needs_mem = false;
    let mut namespaces: Vec<String> = vec![];
    for imp in m.imports() {
        let ty = match imp.ty() {
            ExternType::Func(f) => func_sig(&f),
            ExternType::Memory(t) => {
                format!("(memory min={} max={:?})", t.minimum(), t.maximum())
            }
            ExternType::Global(_) => "(global)".into(),
            ExternType::Table(_) => "(table)".into(),
            _ => "(?)".into(),
        };
        println!("  {}.{} : {ty}", imp.module(), imp.name());
        if imp.module() == "env" && imp.name() == "memory" {
            needs_mem = true;
        } else if !namespaces.contains(&imp.module().to_string()) {
            namespaces.push(imp.module().to_string());
        }
    }
    println!("exports:");
    for exp in m.exports() {
        println!("  {}", exp.name());
    }
    println!("needs:");
    println!(
        "  shared env.memory: {}",
        if needs_mem { "yes" } else { "no (own memory)" }
    );
    println!("  namespaces: {}", namespaces.join(", "));
    Ok(())
}
