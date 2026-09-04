//! End-to-end CLI tests. Every assertion runs the real binary against real
//! files, because that is the only thing that proves the harness works: a
//! module that assembles is not the same as a module that validates and runs.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::Mutex;

/// The repository examples are shared mutable state: a stale `.wat` makes any
/// command rebuild the tracked `.wasm`, and two tests rebuilding the same
/// artifact at once can read a half-written file. Tests that touch them run one
/// at a time.
static EXAMPLES: Mutex<()> = Mutex::new(());

fn examples_lock() -> std::sync::MutexGuard<'static, ()> {
    EXAMPLES
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn host_rs() -> &'static str {
    env!("CARGO_BIN_EXE_host-rs")
}

/// Repository root: the harness crate lives one level below it.
fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("host-rs/ has a parent")
        .to_path_buf()
}

/// A clean directory per test. Tests run in parallel, so the name must be
/// unique per test function.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join("host-rs-tests").join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

fn run(dir: &Path, args: &[&str]) -> Output {
    Command::new(host_rs())
        .args(args)
        .current_dir(dir)
        .output()
        .expect("spawn host-rs")
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

/// Scaffold a native project in a fresh directory and return the project dir.
fn scaffold(name: &str) -> PathBuf {
    scaffold_target(name, "native")
}

/// Scaffold a project for `target` in a fresh directory and return its dir.
fn scaffold_target(name: &str, target: &str) -> PathBuf {
    let dir = scratch(name);
    let mut child = Command::new(host_rs())
        .args(["new", "app"])
        .current_dir(&dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn host-rs new");
    child
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(format!("{target}\n").as_bytes())
        .expect("choose the target");
    let out = child.wait_with_output().expect("host-rs new finished");
    assert!(out.status.success(), "new failed: {}", stderr(&out));
    dir.join("app")
}

/// Replace the root module's closing paren with an include of `fragment`.
fn include_fragment(project: &Path, relative: &str, body: &str) {
    let root = project.join("app.wat");
    let source = std::fs::read_to_string(&root).expect("read root wat");
    let trimmed = source.trim_end();
    let module = trimmed
        .strip_suffix(')')
        .expect("root wat ends with the module's closing paren");
    std::fs::write(&root, format!("{module}  ;; @include {relative}\n)\n"))
        .expect("write root wat");
    let fragment = project.join(relative);
    if let Some(parent) = fragment.parent() {
        std::fs::create_dir_all(parent).expect("create fragment dir");
    }
    std::fs::write(fragment, body).expect("write fragment");
}

#[test]
fn scaffold_builds_checks_and_runs() {
    let project = scaffold("scaffold");
    let built = run(&project, &["build"]);
    assert!(built.status.success(), "build failed: {}", stderr(&built));

    let checked = run(&project, &["check"]);
    assert!(
        checked.status.success(),
        "check failed: {}",
        stderr(&checked)
    );
    assert!(stdout(&checked).contains("all imports satisfied"));

    let ran = run(&project, &["run"]);
    assert!(ran.status.success(), "run failed: {}", stderr(&ran));
    assert_eq!(stdout(&ran), "hello from app\n");
}

#[test]
fn build_rejects_an_invalid_module_and_keeps_the_previous_artifact() {
    let project = scaffold("invalid-module");
    let artifact = project.join("app.wasm");
    let good = std::fs::read(&artifact).expect("scaffold wrote an artifact");

    // A type error: `i32.add` has one operand, and the function returns nothing.
    include_fragment(
        &project,
        "src/broken.wat",
        ";; broken fragment\n(func (export \"oops\") (i32.add (i32.const 1)))\n",
    );

    let out = run(&project, &["build"]);
    assert!(!out.status.success(), "build accepted an invalid module");
    assert_eq!(
        std::fs::read(&artifact).expect("artifact still present"),
        good,
        "a failed build must not overwrite the previous artifact"
    );
}

#[test]
fn validation_error_points_at_the_authored_fragment() {
    let project = scaffold("validation-origin");
    include_fragment(
        &project,
        "src/broken.wat",
        ";; broken fragment\n(func (export \"oops\") (i32.add (i32.const 1)))\n",
    );

    let out = run(&project, &["build"]);
    let message = stderr(&out);
    assert!(!out.status.success());
    assert!(
        message.contains("src/broken.wat:2"),
        "validation error must name the fragment and line, got: {message}"
    );
}

#[test]
fn assembly_error_points_at_the_authored_fragment() {
    let project = scaffold("assembly-origin");
    include_fragment(
        &project,
        "src/broken.wat",
        ";; broken fragment\n(func (export \"oops\") (result i32) (i32.const \"x\"))\n",
    );

    let out = run(&project, &["build"]);
    let message = stderr(&out);
    assert!(!out.status.success());
    assert!(
        message.contains("src/broken.wat:2:"),
        "assembly error must name the fragment, line, and column, got: {message}"
    );
}

#[test]
fn nested_includes_are_expanded() {
    let project = scaffold("nested-include");
    include_fragment(
        &project,
        "src/outer.wat",
        ";; outer fragment\n;; @include src/inner.wat\n",
    );
    std::fs::write(
        project.join("src/inner.wat"),
        "(func (export \"deep\") (result i32) (i32.const 7))\n",
    )
    .expect("write inner fragment");

    let out = run(&project, &["build"]);
    assert!(
        out.status.success(),
        "nested include failed: {}",
        stderr(&out)
    );

    let checked = run(&project, &["check"]);
    assert!(checked.status.success(), "{}", stderr(&checked));
}

#[test]
fn include_cycles_are_rejected() {
    let project = scaffold("include-cycle");
    include_fragment(&project, "src/a.wat", ";; @include src/b.wat\n");
    std::fs::write(project.join("src/b.wat"), ";; @include src/a.wat\n")
        .expect("write cyclic fragment");

    let out = run(&project, &["build"]);
    assert!(!out.status.success(), "a cycle must not build");
    assert!(stderr(&out).contains("cycle"), "{}", stderr(&out));
}

#[test]
fn includes_may_not_escape_the_project() {
    let project = scaffold("include-escape");
    include_fragment(&project, "src/a.wat", ";; @include ../escape.wat\n");

    let out = run(&project, &["build"]);
    assert!(!out.status.success(), "`..` include must be rejected");
    assert!(stderr(&out).contains("project-local"), "{}", stderr(&out));
}

#[test]
fn missing_includes_are_rejected() {
    let project = scaffold("include-missing");
    include_fragment(&project, "src/a.wat", ";; @include src/nope.wat\n");

    let out = run(&project, &["build"]);
    assert!(!out.status.success(), "a missing include must be rejected");
    assert!(stderr(&out).contains("is not a file"), "{}", stderr(&out));
}

#[test]
fn repository_examples_check() {
    let _shared = examples_lock();
    let repo = repo();
    let manifests = [
        "examples/hello/hello.toml",
        "examples/pi/pi.toml",
        "examples/server/manifest.toml",
        "examples/server/mt.toml",
        "examples/prompts/prompts.toml",
        "examples/prompts-raw/prompts-raw.toml",
        "examples/gui-hello/host.toml",
        "examples/component-hello/host.toml",
    ];
    for manifest in manifests {
        let out = run(&repo, &["check", manifest]);
        assert!(
            out.status.success(),
            "check {manifest} failed: {}",
            stderr(&out)
        );
    }
}

#[test]
fn pi_example_prints_the_requested_digits() {
    let _shared = examples_lock();
    let mut child = Command::new(host_rs())
        .args(["run", "examples/pi/pi.toml"])
        .current_dir(repo())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn pi example");
    child
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(b"20\n")
        .expect("write digit count");
    let out = child.wait_with_output().expect("pi example finished");
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(
        stdout(&out).contains("3.14159265358979323846"),
        "unexpected pi output: {}",
        stdout(&out)
    );
}

#[test]
fn component_example_prints_its_greeting() {
    let _shared = examples_lock();
    let out = run(&repo(), &["run", "examples/component-hello/host.toml"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(stdout(&out), "hello from AI-direct IR\n");
}

#[test]
fn hello_example_prints_its_greeting() {
    let _shared = examples_lock();
    let out = run(&repo(), &["run", "examples/hello/hello.toml"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(stdout(&out), "hello from AI-direct IR\n");
}

/// Replace the scaffold's root module with `wat`.
fn write_app(project: &Path, wat: &str) {
    std::fs::write(project.join("app.wat"), wat).expect("write root wat");
}

/// A module that prints one named data segment, so its length is whatever the
/// harness computed rather than whatever the author last typed.
fn printer(text: &str) -> String {
    format!(
        r#"(module
  (import "wasi_snapshot_preview1" "fd_write"
    (func $fd_write (param i32 i32 i32 i32) (result i32)))
  (memory 1)
  (export "memory" (memory 0))
  (func (export "_start")
    (i32.store (i32.const 0) (global.get $msg.ptr))
    (i32.store (i32.const 4) (global.get $msg.len))
    (drop (call $fd_write (i32.const 1) (i32.const 0) (i32.const 1) (i32.const 8))))
  (data $msg (i32.const 0x1000) "{text}")
)
"#
    )
}

#[test]
fn named_data_segments_supply_their_own_pointer_and_length() {
    let project = scaffold("named-data");
    // The WAT carries an escape and a multi-byte character; neither is a byte
    // the author counts.
    write_app(&project, &printer(r"caf\u{e9} \1b ok\n"));

    let built = run(&project, &["build"]);
    assert!(built.status.success(), "{}", stderr(&built));
    let ran = run(&project, &["run"]);
    assert!(ran.status.success(), "{}", stderr(&ran));
    assert_eq!(stdout(&ran), "café \u{1b} ok\n");
}

#[test]
fn named_data_length_follows_the_text() {
    let project = scaffold("named-data-edit");
    write_app(&project, &printer(r"hi\n"));
    assert!(run(&project, &["build"]).status.success());
    assert_eq!(stdout(&run(&project, &["run"])), "hi\n");

    // The only edit is the text. Nothing restates its length.
    write_app(&project, &printer(r"a much longer greeting\n"));
    assert!(run(&project, &["build"]).status.success());
    assert_eq!(stdout(&run(&project, &["run"])), "a much longer greeting\n");
}

#[test]
fn overlapping_named_data_segments_are_rejected() {
    let project = scaffold("named-data-overlap");
    write_app(
        &project,
        r#"(module
  (memory 1)
  (data $first (i32.const 0x1000) "0123456789")
  (data $second (i32.const 0x1005) "collides")
)
"#,
    );
    let out = run(&project, &["build"]);
    assert!(!out.status.success(), "overlap must be rejected");
    assert!(stderr(&out).contains("overlaps"), "{}", stderr(&out));
}

#[test]
fn named_data_requires_a_literal_offset() {
    let project = scaffold("named-data-offset");
    write_app(
        &project,
        r#"(module
  (memory 1)
  (global $base i32 (i32.const 4096))
  (data $msg (global.get $base) "x")
)
"#,
    );
    let out = run(&project, &["build"]);
    assert!(!out.status.success(), "a computed offset must be rejected");
    assert!(stderr(&out).contains("literal"), "{}", stderr(&out));
}

#[test]
fn rebuild_progress_stays_off_the_application_stdout() {
    let project = scaffold("progress-stream");
    write_app(&project, &printer(r"only this\n"));
    assert!(run(&project, &["build"]).status.success());

    // Rewriting the source makes it newer, so `run` rebuilds before executing.
    write_app(&project, &printer(r"only this\n"));
    let ran = run(&project, &["run"]);
    assert!(ran.status.success(), "{}", stderr(&ran));
    assert_eq!(stdout(&ran), "only this\n");
    assert!(
        stderr(&ran).contains("built"),
        "progress belongs on stderr: {}",
        stderr(&ran)
    );
}

#[test]
fn component_scaffold_builds_checks_and_runs() {
    let project = scaffold_target("component-scaffold", "component");

    let built = run(&project, &["build"]);
    assert!(built.status.success(), "build failed: {}", stderr(&built));

    let checked = run(&project, &["check"]);
    assert!(
        checked.status.success(),
        "check failed: {}",
        stderr(&checked)
    );
    let report = stdout(&checked);
    assert!(report.contains("wasi:cli/run@"), "{report}");
    assert!(report.contains("all imports satisfied"), "{report}");

    // The starter is a hand-written WASI 0.2 component: no bindings generator,
    // no language toolchain, and no hand-counted string length.
    let ran = run(&project, &["run"]);
    assert!(ran.status.success(), "run failed: {}", stderr(&ran));
    assert_eq!(stdout(&ran), "hello from app\n");
}

#[test]
fn component_artifact_is_a_component_not_a_core_module() {
    let project = scaffold_target("component-artifact", "component");
    assert!(run(&project, &["build"]).status.success());
    let bytes = std::fs::read(project.join("app.wasm")).expect("read artifact");
    // Layer 1 in the WASM preamble is what distinguishes a component.
    assert_eq!(&bytes[..4], b"\0asm");
    assert_eq!(&bytes[4..6], &[0x0d, 0x00], "artifact is not a component");
}

#[test]
fn a_core_module_is_rejected_by_the_component_target() {
    let project = scaffold_target("component-core-module", "component");
    // A Core module where the manifest promises a component.
    write_app(
        &project,
        "(module (func (export \"run\") (result i32) (i32.const 0)))\n",
    );
    let out = run(&project, &["build"]);
    assert!(!out.status.success(), "a Core module must be rejected");
    assert!(
        stderr(&out).contains("component") || stderr(&out).contains("expected"),
        "{}",
        stderr(&out)
    );
}

#[test]
fn component_distribution_bundles_the_host() {
    let project = scaffold_target("component-dist", "component");
    let out = run(&project, &["dist"]);
    assert!(out.status.success(), "dist failed: {}", stderr(&out));
    let dist = project.join("dist");
    for name in ["host-rs", "host.toml", "app.wasm"] {
        assert!(dist.join(name).is_file(), "dist is missing {name}");
    }
    let manifest = std::fs::read_to_string(dist.join("host.toml")).expect("read dist manifest");
    assert!(manifest.contains("component"), "{manifest}");
}
