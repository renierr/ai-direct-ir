//! Released provider packages: install them into a content-addressed local
//! store, pin them in `air.lock`, and resolve only that lock at runtime.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

use crate::{
    fail,
    manifest::{Provider, is_component_binary},
};

#[derive(Deserialize)]
struct PackageFile {
    package: PackageMeta,
    artifacts: Vec<Artifact>,
}

#[derive(Deserialize)]
struct PackageMeta {
    name: String,
    version: String,
    license: String,
    wit: String,
}

#[derive(Deserialize)]
struct Artifact {
    kind: String,
    target: String,
    path: String,
    sha256: String,
}

#[derive(Deserialize)]
struct ManifestProviders {
    #[serde(default)]
    providers: Vec<ManifestProvider>,
}

#[derive(Deserialize)]
struct ManifestProvider {
    package: Option<String>,
}

#[derive(Default, Deserialize, Serialize)]
struct Lockfile {
    #[serde(default)]
    provider: Vec<LockedProvider>,
}

#[derive(Clone, Deserialize, Serialize)]
struct LockedProvider {
    package: String,
    version: String,
    target: String,
    sha256: String,
    wit_sha256: String,
    metadata_sha256: String,
    license: String,
    source: String,
    /// Path inside the package store, never an absolute host path. The store
    /// root is derived from `sha256`, keeping committed locks portable.
    artifact: String,
}

fn lock_path(base: &Path) -> PathBuf {
    base.join("air.lock")
}

fn store() -> PathBuf {
    if let Some(cache) = std::env::var_os("XDG_CACHE_HOME") {
        return PathBuf::from(cache).join("air/providers");
    }
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".cache/air/providers")
}

pub fn hash(path: &Path) -> wasmtime::Result<String> {
    let bytes = std::fs::read(path)?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

fn load_lock(base: &Path) -> wasmtime::Result<Lockfile> {
    let path = lock_path(base);
    if !path.is_file() {
        return Ok(Lockfile::default());
    }
    Ok(toml::from_str(&std::fs::read_to_string(path)?)?)
}

/// Turn every package declaration into the verified absolute path the existing
/// builder/linker already understands. Local path providers remain untouched.
pub fn resolve(base: &Path, providers: &mut [Provider]) -> wasmtime::Result<()> {
    let lock = load_lock(base)?;
    for provider in providers {
        let (Some(package), Some(version)) = (&provider.package, &provider.version) else {
            if provider.path.is_empty() {
                return fail(
                    "a [[providers]] entry needs `path`, or both `package` and `version`".into(),
                );
            }
            continue;
        };
        if provider.source.is_some() || !provider.path.is_empty() {
            return fail(format!(
                "provider `{package}` uses package resolution and may not also declare `source` or `path`"
            ));
        }
        let entry = lock.provider.iter().find(|entry| {
            entry.package == *package && entry.version == *version
        }).ok_or_else(|| wasmtime::Error::msg(format!(
            "provider `{package}@{version}` is not locked; restore it with `air add --from <package-dir> {package}@{version}`"
        )))?;
        let root = store().join(&entry.sha256);
        let artifact = root.join(&entry.artifact);
        if !artifact.is_file() {
            return fail(format!(
                "provider `{package}@{version}` is missing from the local store; restore it with `air add --from <package-dir> {package}@{version}`"
            ));
        }
        let actual = hash(&artifact)?;
        if actual != entry.sha256 {
            return fail(format!(
                "provider `{package}@{version}` hash mismatch: lock has {}, store has {actual}",
                entry.sha256
            ));
        }
        let metadata = root.join("provider.toml");
        if hash(&metadata)? != entry.metadata_sha256 {
            return fail(format!(
                "provider `{package}@{version}` metadata hash mismatch in the local store"
            ));
        }
        let parsed: PackageFile = toml::from_str(&std::fs::read_to_string(&metadata)?)?;
        if parsed.package.name != *package
            || parsed.package.version != *version
            || parsed.package.license != entry.license
        {
            return fail(format!(
                "provider `{package}@{version}` metadata does not match air.lock"
            ));
        }
        if hash(&root.join(parsed.package.wit))? != entry.wit_sha256 {
            return fail(format!(
                "provider `{package}@{version}` WIT hash mismatch in the local store"
            ));
        }
        provider.path = artifact.to_string_lossy().into_owned();
    }
    Ok(())
}

/// Write provenance for the package artifacts copied into a release bundle.
/// The distributed manifest uses direct paths, so this is an audit record, not
/// a second resolver input.
pub fn distribution_lock(
    base: &Path,
    providers: &[Provider],
    paths: &[String],
) -> wasmtime::Result<Option<String>> {
    let lock = load_lock(base)?;
    let mut out = Lockfile::default();
    for (provider, path) in providers.iter().zip(paths) {
        let (Some(package), Some(version)) = (&provider.package, &provider.version) else {
            continue;
        };
        let mut entry = lock
            .provider
            .iter()
            .find(|entry| entry.package == *package && entry.version == *version)
            .cloned()
            .ok_or_else(|| {
                wasmtime::Error::msg(format!(
                    "provider `{package}@{version}` disappeared from air.lock during distribution"
                ))
            })?;
        entry.artifact = path.clone();
        out.provider.push(entry);
    }
    (!out.provider.is_empty())
        .then(|| toml::to_string_pretty(&out).map_err(Into::into))
        .transpose()
}

/// Install one already-reviewed package without networking. The lock contains
/// all facts needed to verify and locate it on later invocations.
pub fn add(source: &str, requested: &str) -> wasmtime::Result<()> {
    let (name, version) = requested
        .rsplit_once('@')
        .ok_or_else(|| wasmtime::Error::msg("provider must be written as <package>@<version>"))?;
    if name.is_empty() || version.is_empty() {
        return fail("provider must be written as <package>@<version>".into());
    }
    let source = std::fs::canonicalize(source)?;
    let metadata = source.join("provider.toml");
    let parsed: PackageFile = toml::from_str(&std::fs::read_to_string(&metadata)?)?;
    if parsed.package.name != name || parsed.package.version != version {
        return fail(format!(
            "requested `{requested}`, but `{}` declares {}@{}",
            metadata.display(),
            parsed.package.name,
            parsed.package.version
        ));
    }
    let artifact = parsed
        .artifacts
        .iter()
        .find(|a| a.kind == "component" && a.target == "wasm32-wasi")
        .ok_or_else(|| {
            wasmtime::Error::msg(format!(
                "`{}` has no wasm32-wasi component artifact",
                metadata.display()
            ))
        })?;
    let artifact_source = source.join(&artifact.path);
    let artifact_hash = hash(&artifact_source)?;
    if artifact_hash != artifact.sha256 {
        return fail(format!(
            "provider `{requested}` artifact hash mismatch: provider.toml has {}, file has {artifact_hash}",
            artifact.sha256
        ));
    }
    let bytes = std::fs::read(&artifact_source)?;
    if !is_component_binary(&bytes) {
        return fail(format!(
            "provider `{requested}` artifact is a Core WASM module, not a component"
        ));
    }
    let wit_hash = hash(&source.join(&parsed.package.wit))?;
    let metadata_hash = hash(&metadata)?;
    let installed = store().join(&artifact_hash);
    if !installed.exists() {
        copy_tree(&source, &installed)?;
    }
    let base = std::env::current_dir()?;
    let manifest = base.join("host.toml");
    if !manifest.is_file() {
        return fail(
            "no host.toml in this directory; run `air add` from a project directory".into(),
        );
    }
    let mut lock = load_lock(&base)?;
    lock.provider
        .retain(|entry| entry.package != name || entry.version != version);
    lock.provider.push(LockedProvider {
        package: name.into(),
        version: version.into(),
        target: artifact.target.clone(),
        sha256: artifact_hash,
        wit_sha256: wit_hash,
        metadata_sha256: metadata_hash,
        license: parsed.package.license,
        source: "local".into(),
        artifact: artifact.path.clone(),
    });
    lock.provider
        .sort_by(|a, b| (&a.package, &a.version).cmp(&(&b.package, &b.version)));
    std::fs::write(lock_path(&base), toml::to_string_pretty(&lock)?)?;
    let text = std::fs::read_to_string(&manifest)?;
    let declaration = format!("\n[[providers]]\npackage = {name:?}\nversion = {version:?}\n");
    let already_declared = text.contains(&format!("package = {name:?}\nversion = {version:?}"));
    if !already_declared {
        let parsed_manifest: ManifestProviders = toml::from_str(&text)?;
        if parsed_manifest
            .providers
            .iter()
            .any(|provider| provider.package.is_none())
        {
            return fail("host.toml already has a local [[providers]] entry; replace it with the package declaration before running `air add`".into());
        }
        std::fs::write(manifest, format!("{text}{declaration}"))?;
    }
    println!("added {requested}");
    Ok(())
}

fn copy_tree(from: &Path, to: &Path) -> wasmtime::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let dest = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_tree(&entry.path(), &dest)?;
        } else {
            std::fs::copy(entry.path(), dest)?;
        }
    }
    Ok(())
}
