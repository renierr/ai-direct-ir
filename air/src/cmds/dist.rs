//! `air dist` -- bundle a project and the host into a runnable directory.

use wasmtime::{Engine, Result};

use crate::manifest::Target;

use crate::fail;

use super::build::build_if_needed;
use super::{cmd_check, manifest_base, manifest_path};

/// Create a clean, relocatable distribution beside the manifest. Native
/// bundles include their executable host; browser bundles use browser assets.
pub fn cmd_dist(path: &str) -> Result<()> {
    if path == "host.toml" && !std::path::Path::new(path).is_file() {
        return fail(
            "no host.toml in this directory; run `air dist <manifest.toml>` or change to a project directory"
                .into(),
        );
    }
    let manifest = crate::manifest::load(path)?;
    // A release uses current source when the manifest declares it. Prebuilt
    // modules remain valid: they have no source and are checked as supplied.
    let engine = Engine::default();
    build_if_needed(&engine, path, &manifest)?;
    cmd_check(&engine, path, &manifest)?;
    let base = std::fs::canonicalize(manifest_base(path))?;
    let dist = base.join("dist");
    if dist.exists() {
        std::fs::remove_dir_all(&dist)?;
    }
    std::fs::create_dir(&dist)?;

    let app = copy_bundle_file(&base, &dist, &manifest.app.path)?;
    let mut manifest_out = toml::Table::new();
    manifest_out.insert(
        "target".into(),
        toml::Value::String(
            match manifest.target {
                Target::Native => "native",
                Target::Browser => "browser",
                Target::Component => "component",
            }
            .into(),
        ),
    );
    manifest_out.insert(
        "mode".into(),
        toml::Value::String(
            match manifest.mode {
                crate::manifest::Mode::Command => "command",
                crate::manifest::Mode::Gui => "gui",
            }
            .into(),
        ),
    );
    // A grant is part of the application, not of the shell that built it: a
    // distribution that dropped `network = true` would answer `access-denied`
    // to every socket call, the same way one that dropped `root` would find
    // no files.
    if manifest.network {
        manifest_out.insert("network".into(), toml::Value::Boolean(true));
    }
    for (key, value) in [
        (
            "port",
            manifest.port.map(|v| toml::Value::Integer(v.into())),
        ),
        (
            "memory_pages",
            manifest
                .memory_pages
                .map(|v| toml::Value::Integer(v.into())),
        ),
    ] {
        if let Some(value) = value {
            manifest_out.insert(key.into(), value);
        }
    }
    if let Some(root) = &manifest.root {
        // A `root` travels only if it is a directory the bundle can contain.
        // `root = "../.."` grants the whole repository and `root = "."` the
        // project the bundle is being written into: both are development
        // conveniences, and neither is a thing to copy. Resolve first --
        // `examples/app/../..` has no file name at all, which is what used to
        // surface as an unexplained error.
        let source = std::fs::canonicalize(manifest_path(&base, root))
            .unwrap_or_else(|_| manifest_path(&base, root));
        let contains_bundle = dist.starts_with(&source);
        let inside_project = source.starts_with(&base);
        match source.file_name() {
            Some(name) if inside_project && !contains_bundle => {
                copy_dir(&source, &dist.join(name))?;
                manifest_out.insert(
                    "root".into(),
                    toml::Value::String(name.to_string_lossy().into()),
                );
                if let Some(guest) = &manifest.guest {
                    manifest_out.insert("guest".into(), toml::Value::String(guest.clone()));
                }
            }
            _ => {
                // Dropping the grant narrows what the packaged app can reach,
                // which is the safe direction -- but it changes how the app is
                // run, so say so rather than leaving it to be discovered.
                eprintln!(
                    "note: `root = \"{root}\"` resolves to {} and cannot travel \
                     with the bundle; the packaged app grants no directory. \
                     Run it with `air run --dir <path> host.toml`.",
                    source.display()
                );
            }
        }
    }
    if matches!(manifest.target, Target::Native | Target::Component) {
        let mut libs = Vec::new();
        for lib in &manifest.libs {
            let path = copy_bundle_file(&base, &dist, &lib.path)?;
            let mut item = toml::Table::new();
            item.insert("path".into(), toml::Value::String(path));
            item.insert("as".into(), toml::Value::String(lib.namespace.clone()));
            libs.push(toml::Value::Table(item));
        }
        if !libs.is_empty() {
            manifest_out.insert("libs".into(), toml::Value::Array(libs));
        }
        let mut bridges = Vec::new();
        for bridge in &manifest.bridges {
            let path = copy_bundle_file(&base, &dist, &bridge.path)?;
            let mut item = toml::Table::new();
            item.insert("path".into(), toml::Value::String(path));
            item.insert("as".into(), toml::Value::String(bridge.namespace.clone()));
            item.insert("alloc".into(), toml::Value::String(bridge.alloc.clone()));
            let calls = bridge
                .calls
                .iter()
                .map(|call| {
                    let mut call_out = toml::Table::new();
                    call_out.insert("as".into(), toml::Value::String(call.name.clone()));
                    call_out.insert("func".into(), toml::Value::String(call.func.clone()));
                    call_out.insert("in_ptr".into(), toml::Value::Integer(call.in_ptr as i64));
                    call_out.insert("in_len".into(), toml::Value::Integer(call.in_len as i64));
                    call_out.insert("out_ptr".into(), toml::Value::Integer(call.out_ptr as i64));
                    call_out.insert("out_len".into(), toml::Value::Integer(call.out_len.into()));
                    call_out.insert("max_in".into(), toml::Value::Integer(call.max_in.into()));
                    toml::Value::Table(call_out)
                })
                .collect();
            item.insert("calls".into(), toml::Value::Array(calls));
            bridges.push(toml::Value::Table(item));
        }
        if !bridges.is_empty() {
            manifest_out.insert("bridges".into(), toml::Value::Array(bridges));
        }
        // A component's providers are part of the application the same way a
        // Core lib is: the artifact does not contain them, it imports their
        // interfaces, so a distribution without them cannot instantiate.
        let mut providers = Vec::new();
        for provider in &manifest.providers {
            let path = copy_bundle_file(&base, &dist, &provider.path)?;
            let mut item = toml::Table::new();
            item.insert("path".into(), toml::Value::String(path));
            providers.push(toml::Value::Table(item));
        }
        if !providers.is_empty() {
            manifest_out.insert("providers".into(), toml::Value::Array(providers));
        }
        let executable = std::env::current_exe()?;
        let host_name = if cfg!(windows) { "air.exe" } else { "air" };
        std::fs::copy(executable, dist.join(host_name))?;
    } else {
        for name in ["index.html", "web-host.js"] {
            let source = base.join(name);
            if !source.is_file() {
                return fail(format!(
                    "browser distribution requires `{}`",
                    source.display()
                ));
            }
            std::fs::copy(source, dist.join(name))?;
        }
    }
    let mut app_out = toml::Table::new();
    app_out.insert("path".into(), toml::Value::String(app));
    app_out.insert("run".into(), toml::Value::String(manifest.app.run));
    manifest_out.insert("app".into(), toml::Value::Table(app_out));
    std::fs::write(
        dist.join("host.toml"),
        toml::to_string_pretty(&manifest_out)?,
    )?;
    println!(
        "created {} distribution at {}",
        match manifest.target {
            Target::Native => "native",
            Target::Browser => "browser",
            Target::Component if manifest.mode == crate::manifest::Mode::Gui => "GUI component",
            Target::Component => "component",
        },
        dist.display()
    );
    Ok(())
}

/// Copy a manifest-relative file under `dist/` without allowing a source path
/// outside the project to dictate an unsafe destination path.
fn copy_bundle_file(
    base: &std::path::Path,
    dist: &std::path::Path,
    source: &str,
) -> Result<String> {
    let source = std::fs::canonicalize(manifest_path(base, source))?;
    let name = source
        .file_name()
        .ok_or_else(|| wasmtime::Error::msg("module has no file name"))?
        .to_string_lossy()
        .into_owned();
    let dest = dist.join(&name);
    std::fs::copy(&source, &dest)?;
    Ok(name)
}

fn copy_dir(source: &std::path::Path, dest: &std::path::Path) -> Result<()> {
    if !source.is_dir() {
        return fail(format!("`{}` is not a directory", source.display()));
    }
    std::fs::create_dir_all(dest)?;
    for entry in std::fs::read_dir(source)? {
        let entry = entry?;
        let child = dest.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &child)?;
        } else {
            std::fs::copy(entry.path(), child)?;
        }
    }
    Ok(())
}
