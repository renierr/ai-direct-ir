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

fn air_bin() -> &'static str {
    env!("CARGO_BIN_EXE_air")
}

/// Repository root: the harness crate lives one level below it.
fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("air/ has a parent")
        .to_path_buf()
}

/// A clean directory per test. Tests run in parallel, so the name must be
/// unique per test function.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join("air-tests").join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

fn run(dir: &Path, args: &[&str]) -> Output {
    Command::new(air_bin())
        .args(args)
        .current_dir(dir)
        .output()
        .expect("spawn air")
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
    let mut child = Command::new(air_bin())
        .args(["new", "app"])
        .current_dir(&dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn air new");
    child
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(format!("{target}\n").as_bytes())
        .expect("choose the target");
    let out = child.wait_with_output().expect("air new finished");
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
        "examples/hello/host.toml",
        "examples/pi/host.toml",
        "examples/prompts/host.toml",
        "examples/server/manifest.toml",
        "examples/server/mt.toml",
        "examples/prompts-raw/host.toml",
        "examples/gui-hello/host.toml",
        "examples/provider-demo/host.toml",
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
fn prompts_example_runs_scripted() {
    let _shared = examples_lock();
    let mut child = Command::new(air_bin())
        .args(["run", "examples/prompts/host.toml"])
        .current_dir(repo())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn prompts example");
    child
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(b"myproj\n2\n1 3\ny\n")
        .expect("write scripted answers");
    let out = child.wait_with_output().expect("prompts example finished");
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(
        stdout(&out).contains("Done: myproj on staging"),
        "unexpected prompts output: {}",
        stdout(&out)
    );
}

#[test]
fn pi_example_prints_the_requested_digits() {
    let _shared = examples_lock();
    let mut child = Command::new(air_bin())
        .args(["run", "examples/pi/host.toml"])
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
fn provider_example_calls_across_the_component_boundary() {
    let _shared = examples_lock();
    // The string crosses consumer -> host -> provider -> host -> consumer, all
    // wired at link time with no build-time composition tool.
    let out = run(&repo(), &["run", "examples/provider-demo/host.toml"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(stdout(&out), "HELLO FROM A PROVIDER\n");
}

#[test]
fn a_core_module_is_rejected_as_a_provider() {
    let project = scaffold_target("provider-core-module", "component");
    // A Core module where the manifest declares a component provider.
    std::fs::write(
        project.join("provider.wat"),
        "(module (func (export \"noop\")))\n",
    )
    .expect("write provider source");
    let manifest = project.join("host.toml");
    let mut text = std::fs::read_to_string(&manifest).expect("read manifest");
    text.push_str("\n[[providers]]\nsource = \"provider.wat\"\npath = \"provider.wasm\"\n");
    std::fs::write(&manifest, text).expect("write manifest");

    let out = run(&project, &["build"]);
    assert!(!out.status.success(), "a Core provider must be rejected");
    assert!(
        stderr(&out).contains("Core WASM module"),
        "{}",
        stderr(&out)
    );
}

#[test]
fn hello_example_prints_its_greeting() {
    let _shared = examples_lock();
    // hello is a WASI 0.2 component: the greeting and its exact length come
    // from the component, not from a hand-counted constant.
    let out = run(&repo(), &["run", "examples/hello/host.toml"]);
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
fn the_target_is_inferred_when_the_manifest_omits_it() {
    let project = scaffold_target("target-inference", "component");
    let manifest = project.join("host.toml");
    let declared = std::fs::read_to_string(&manifest).expect("read manifest");
    let without_target: String = declared
        .lines()
        .filter(|line| !line.starts_with("target"))
        .map(|line| format!("{line}\n"))
        .collect();
    assert!(!without_target.contains("target"));
    std::fs::write(&manifest, without_target).expect("write manifest");

    // The artifact's own preamble says it is a component; nothing else has to.
    let checked = run(&project, &["check"]);
    assert!(checked.status.success(), "{}", stderr(&checked));
    assert!(
        stdout(&checked).contains("wasi:cli/run@"),
        "{}",
        stdout(&checked)
    );
    let ran = run(&project, &["run"]);
    assert!(ran.status.success(), "{}", stderr(&ran));
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
    for name in ["air", "host.toml", "app.wasm"] {
        assert!(dist.join(name).is_file(), "dist is missing {name}");
    }
    let manifest = std::fs::read_to_string(dist.join("host.toml")).expect("read dist manifest");
    assert!(manifest.contains("component"), "{manifest}");
}

#[test]
fn a_component_can_import_the_projects_own_interface() {
    let project = scaffold_target("component-term", "component");
    // `ai-direct:host/term` is not WASI. A component imports a project-owned
    // interface exactly as it imports `wasi:io/streams`.
    let starter = std::fs::read_to_string(project.join("app.wat")).expect("read starter");
    let boundary = starter
        .split(";; --- application logic")
        .next()
        .expect("starter has an application section");
    std::fs::write(
        project.join("app.wat"),
        format!(
            r#"{boundary}
  (import "ai-direct:host/term" (instance $term
    (export "available" (func (result s32)))))
  (alias export $term "available" (func $available))
  (core func $available-l (canon lower (func $available)))
  (core instance $t (export "available" (func $available-l)))

  (core module $main
    (import "env" "memory" (memory 1))
    (import "wasi" "get-stdout" (func $get_stdout (result i32)))
    (import "wasi" "write" (func $write (param i32 i32 i32 i32)))
    (import "term" "available" (func $available (result i32)))
    (data $no (i32.const 0x100) "terminal: no\n")
    (data $yes (i32.const 0x120) "terminal: yes\n")
    (func (export "run") (result i32)
      (if (call $available)
        (then (call $write (call $get_stdout)
                (global.get $yes.ptr) (global.get $yes.len) (i32.const 0x200)))
        (else (call $write (call $get_stdout)
                (global.get $no.ptr) (global.get $no.len) (i32.const 0x200))))
      (i32.load (i32.const 0x200))))
  (core instance $app (instantiate $main
    (with "env" (instance $mem))
    (with "wasi" (instance $wasi))
    (with "term" (instance $t))))

  (func $run (result (result)) (canon lift (core func $app "run")))
  (instance $run-i (export "run" (func $run)))
  (export "wasi:cli/run@0.2.12" (instance $run-i))
)
"#
        ),
    )
    .expect("write app");

    let checked = run(&project, &["check"]);
    assert!(checked.status.success(), "{}", stderr(&checked));
    let ran = run(&project, &["run"]);
    assert!(ran.status.success(), "{}", stderr(&ran));
    // Tests do not run on a terminal, so the honest answer is "no".
    assert_eq!(stdout(&ran), "terminal: no\n");
}

/// Rewrite the scaffolded component's `;; @wasi` line.
fn set_wasi_directive(project: &Path, args: &str) {
    let root = project.join("app.wat");
    let source = std::fs::read_to_string(&root).expect("read root wat");
    let rewritten: String = source
        .lines()
        .map(|line| {
            if line.trim().starts_with(";; @wasi") {
                format!("  ;; @wasi {args}\n")
            } else {
                format!("{line}\n")
            }
        })
        .collect();
    assert!(rewritten.contains(args), "directive not rewritten");
    std::fs::write(&root, rewritten).expect("write root wat");
}

/// The generated boundary imports what the directive asked for and nothing
/// else. Import names survive into the artifact as literal strings, so the
/// bytes themselves are the evidence.
#[test]
fn the_wasi_directive_imports_only_the_requested_capabilities() {
    let project = scaffold_target("wasi-capabilities", "component");
    let artifact = project.join("app.wasm");

    assert!(run(&project, &["build"]).status.success());
    let bytes = std::fs::read(&artifact).expect("read artifact");
    let names = String::from_utf8_lossy(&bytes).into_owned();
    assert!(names.contains("wasi:cli/stdout@"), "stdout was requested");
    assert!(
        !names.contains("wasi:cli/stdin@"),
        "stdin was not requested"
    );
    assert!(!names.contains("wasi:cli/exit@"), "exit was not requested");

    set_wasi_directive(&project, "stdout stdin exit pages=2");
    let rebuilt = run(&project, &["build"]);
    assert!(
        rebuilt.status.success(),
        "build failed: {}",
        stderr(&rebuilt)
    );
    let bytes = std::fs::read(&artifact).expect("read rebuilt artifact");
    let names = String::from_utf8_lossy(&bytes).into_owned();
    assert!(names.contains("wasi:cli/stdin@"), "stdin was requested");
    assert!(names.contains("wasi:cli/exit@"), "exit was requested");

    // Adding capabilities does not disturb the program written against them.
    let ran = run(&project, &["run"]);
    assert!(ran.status.success(), "run failed: {}", stderr(&ran));
    assert_eq!(stdout(&ran), "hello from app\n");
}

/// A misspelled capability stops the build and names the line that asked for
/// it, rather than silently generating a boundary without it.
#[test]
fn an_unknown_wasi_capability_is_rejected() {
    let project = scaffold_target("wasi-typo", "component");
    set_wasi_directive(&project, "stdout stdinn");

    let built = run(&project, &["build"]);
    assert!(!built.status.success(), "a typo must not build");
    let message = stderr(&built);
    assert!(message.contains("stdinn"), "{message}");
    assert!(message.contains("app.wat"), "{message}");
}

/// Two boundaries would redefine every generated name. Say so directly instead
/// of reporting a duplicate identifier in code the author never wrote.
#[test]
fn a_second_wasi_directive_is_rejected() {
    let project = scaffold_target("wasi-twice", "component");
    let root = project.join("app.wat");
    let source = std::fs::read_to_string(&root).expect("read root wat");
    // Duplicate the directive line itself; the file header mentions it in prose.
    let doubled: String = source
        .lines()
        .map(|line| {
            if line.trim().starts_with(";; @wasi") {
                format!("{line}\n  ;; @wasi stderr\n")
            } else {
                format!("{line}\n")
            }
        })
        .collect();
    std::fs::write(&root, doubled).expect("write root wat");

    let built = run(&project, &["build"]);
    assert!(!built.status.success(), "two boundaries must not build");
    let message = stderr(&built);
    assert!(message.contains("second WASI boundary"), "{message}");
}

/// The boundary is generated, so a validation failure inside the application
/// must still point at the application's own line.
#[test]
fn errors_below_a_generated_boundary_keep_their_line() {
    let project = scaffold_target("wasi-origins", "component");
    let root = project.join("app.wat");
    let source = std::fs::read_to_string(&root).expect("read root wat");
    let broken = source.replace("(i32.load (i32.const 0x200))", "(i32.load (f32.const 0))");
    assert_ne!(broken, source, "the run body was not found");
    std::fs::write(&root, broken).expect("write root wat");

    let built = run(&project, &["build"]);
    assert!(!built.status.success(), "invalid WAT must not build");
    let message = stderr(&built);
    assert!(message.contains("app.wat"), "{message}");
}

/// Replace the scaffold's placed segment with unplaced ones and hand the
/// harness a region to put them in.
fn use_data_region(project: &Path, region: &str, segments: &str) {
    let root = project.join("app.wat");
    let source = std::fs::read_to_string(&root).expect("read root wat");
    let placed = "    (data $msg (i32.const 0x100) \"hello from app\\n\")";
    assert!(source.contains(placed), "scaffold segment not found");
    std::fs::write(
        &root,
        source.replace(placed, &format!("    ;; @data {region}\n{segments}")),
    )
    .expect("write root wat");
}

/// The other hand-maintained number: an author who declares a region stops
/// assigning addresses, and inserting a word into one string no longer moves
/// every string after it.
#[test]
fn unplaced_segments_are_packed_into_the_declared_region() {
    let project = scaffold_target("data-region", "component");
    use_data_region(
        &project,
        "0x1000..0x8000",
        "    (data $msg \"hello from app\\n\")\n    (data $tail \"and one more line\\n\")",
    );
    let root = project.join("app.wat");
    let source = std::fs::read_to_string(&root).expect("read root wat");
    std::fs::write(
        &root,
        source.replace(
            "      (i32.load (i32.const 0x200)))",
            "      (call $write (call $get-stdout)\n\
             \x20       (global.get $tail.ptr) (global.get $tail.len) (i32.const 0x200))\n\
             \x20     (i32.load (i32.const 0x200)))",
        ),
    )
    .expect("write root wat");

    let ran = run(&project, &["run"]);
    assert!(ran.status.success(), "run failed: {}", stderr(&ran));
    // Packed in source order, so both strings print whole and in order.
    assert_eq!(stdout(&ran), "hello from app\nand one more line\n");
}

/// A placed segment keeps its address; the region is the part handed over.
#[test]
fn placed_and_unplaced_segments_coexist() {
    let project = scaffold_target("data-mixed", "component");
    use_data_region(
        &project,
        "0x1000",
        "    (data $msg \"hello from app\\n\")\n    (data $fixed (i32.const 0x400) \"fixed\")",
    );
    let ran = run(&project, &["run"]);
    assert!(ran.status.success(), "run failed: {}", stderr(&ran));
    assert_eq!(stdout(&ran), "hello from app\n");
}

/// Without a region the harness will not guess at an address, because the
/// author is already using memory it cannot see.
#[test]
fn an_unplaced_segment_without_a_region_is_rejected() {
    let project = scaffold_target("data-no-region", "component");
    let root = project.join("app.wat");
    let source = std::fs::read_to_string(&root).expect("read root wat");
    std::fs::write(
        &root,
        source.replace("(data $msg (i32.const 0x100) ", "(data $msg "),
    )
    .expect("write root wat");

    let built = run(&project, &["build"]);
    assert!(!built.status.success(), "must not guess an address");
    let message = stderr(&built);
    assert!(message.contains("@data"), "{message}");
    assert!(message.contains("$msg"), "{message}");
}

/// Overrunning the region is an error, not silent corruption of whatever the
/// author put after it.
#[test]
fn a_region_too_small_for_its_segments_is_rejected() {
    let project = scaffold_target("data-overflow", "component");
    use_data_region(
        &project,
        "0x1000..0x1005",
        "    (data $msg \"hello from app\\n\")",
    );

    let built = run(&project, &["build"]);
    assert!(!built.status.success(), "must not overrun the region");
    let message = stderr(&built);
    assert!(message.contains("does not fit"), "{message}");
}

/// A region that runs into a placed segment is reported against both.
#[test]
fn a_region_over_a_placed_segment_is_rejected() {
    let project = scaffold_target("data-collision", "component");
    use_data_region(
        &project,
        "0x400",
        "    (data $msg \"hello from app\\n\")\n    (data $fixed (i32.const 0x404) \"fixed\")",
    );

    let built = run(&project, &["build"]);
    assert!(
        !built.status.success(),
        "must not place over a fixed segment"
    );
    let message = stderr(&built);
    assert!(message.contains("$fixed"), "{message}");
}

/// `wasi:cli/exit`'s `exit` takes a `result`, so 0 and 1 are the only
/// representable values and anything else traps on the discriminant. The
/// prompts example documents three exit codes, so it asks for `exit-with-code`.
#[test]
fn prompts_example_reports_its_documented_exit_codes() {
    let _guard = examples_lock();
    let manifest = repo().join("examples/prompts/host.toml");
    let manifest = manifest.to_str().expect("utf-8 path");

    for (input, expected, path) in [
        ("my-app\n1\n1\ny\n", 0, "completed"),
        ("my-app\n1\n1\nn\n", 1, "cancelled"),
        ("", 2, "input closed"),
    ] {
        let mut child = Command::new(air_bin())
            .args(["run", manifest])
            .current_dir(repo())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn air run");
        child
            .stdin
            .as_mut()
            .expect("stdin")
            .write_all(input.as_bytes())
            .expect("write answers");
        let out = child.wait_with_output().expect("air run finished");
        assert_eq!(
            out.status.code(),
            Some(expected),
            "{path} path: {}",
            stderr(&out)
        );
    }
}

/// The cancel message ends in a newline. It once did not: `✖` is three UTF-8
/// bytes and the hand-written length counted characters.
#[test]
fn prompts_cancel_message_is_not_truncated() {
    let _guard = examples_lock();
    let manifest = repo().join("examples/prompts/host.toml");
    let mut child = Command::new(air_bin())
        .args(["run", manifest.to_str().expect("utf-8 path")])
        .current_dir(repo())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn air run");
    child
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(b"my-app\n1\n1\nn\n")
        .expect("write answers");
    let out = child.wait_with_output().expect("air run finished");
    let printed = stdout(&out);
    assert!(printed.ends_with("Cancelled.\n"), "{printed:?}");
}

/// The sha256sum example is the end-to-end proof of the provider path: a
/// vendored component built from an upstream crate, a hand-written
/// wasi:filesystem import, and guest arguments forwarded by `air run`.
#[test]
fn sha256sum_example_matches_a_known_digest() {
    let _guard = examples_lock();
    let project = repo().join("examples/sha256sum");

    // "abc" is the FIPS 180-4 vector; the file holds exactly those three bytes.
    let fixture = project.join("abc.txt");
    std::fs::write(&fixture, b"abc").expect("write fixture");

    let out = run(&project, &["run", "host.toml", "abc.txt"]);
    assert!(out.status.success(), "run failed: {}", stderr(&out));
    assert_eq!(
        stdout(&out),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad  abc.txt\n"
    );

    let _ = std::fs::remove_file(&fixture);
}

/// Arguments reach the guest, and the app distinguishes its exit codes.
#[test]
fn sha256sum_example_reports_usage_and_errors() {
    let _guard = examples_lock();
    let project = repo().join("examples/sha256sum");

    let helped = run(&project, &["run", "host.toml", "--help"]);
    assert!(helped.status.success(), "{}", stderr(&helped));
    assert!(stdout(&helped).contains("usage:"), "{}", stdout(&helped));

    // No argument at all: argv reached the guest and it saw only argv[0].
    let bare = run(&project, &["run", "host.toml"]);
    assert_eq!(bare.status.code(), Some(1), "{}", stderr(&bare));
    assert!(
        stderr(&bare).contains("expected one file"),
        "{}",
        stderr(&bare)
    );

    let missing = run(&project, &["run", "host.toml", "nope.txt"]);
    assert_eq!(missing.status.code(), Some(2), "{}", stderr(&missing));
}

/// `--dir` grants a directory the manifest did not, which is what makes a tool
/// usable on files outside its own project. WASI has no global root, so
/// without a grant nothing outside the manifest's `root` is readable.
#[test]
fn dir_grants_a_directory_to_the_guest() {
    let _guard = examples_lock();
    let project = repo().join("examples/sha256sum");
    let manifest = project.join("host.toml");
    let manifest = manifest.to_str().expect("utf-8 path");

    // Run from the repository root, naming a file that is not beside the
    // manifest. Without a grant this fails, and says why.
    let denied = run(&repo(), &["run", manifest, "AGENTS.md"]);
    assert_eq!(denied.status.code(), Some(2), "{}", stderr(&denied));
    let message = stderr(&denied);
    assert!(message.contains("cannot open AGENTS.md"), "{message}");
    assert!(message.contains("--dir"), "{message}");

    let granted = run(&repo(), &["run", "--dir", ".", manifest, "AGENTS.md"]);
    assert!(granted.status.success(), "{}", stderr(&granted));
    let digest = std::process::Command::new("sha256sum")
        .arg("AGENTS.md")
        .current_dir(repo())
        .output()
        .expect("sha256sum");
    let expected = String::from_utf8_lossy(&digest.stdout).into_owned();
    assert_eq!(stdout(&granted), expected);
}

/// `--dir` must come before the manifest, so an application never has to
/// escape its own flags away from the harness's.
#[test]
fn dir_without_a_path_is_rejected() {
    let out = run(&repo(), &["run", "--dir"]);
    assert!(!out.status.success());
    assert!(
        stderr(&out).contains("`--dir` needs a directory"),
        "{}",
        stderr(&out)
    );
}
