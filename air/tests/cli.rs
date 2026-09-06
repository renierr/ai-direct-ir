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

fn run_with_cache(dir: &Path, cache: &Path, args: &[&str]) -> Output {
    Command::new(air_bin())
        .args(args)
        .current_dir(dir)
        .env("XDG_CACHE_HOME", cache)
        .output()
        .expect("spawn air")
}

fn provider_fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/providers")
        .join(name)
}

fn install_example_providers(cache: &Path) {
    let sha256 = provider_fixture("sha256").to_string_lossy().into_owned();
    let out = run_with_cache(
        &repo().join("examples/sha256sum"),
        cache,
        &["add", "--from", &sha256, "ai-direct:sha256@0.1.0"],
    );
    assert!(out.status.success(), "{}", stderr(&out));
    let width = provider_fixture("text-width")
        .to_string_lossy()
        .into_owned();
    let out = run_with_cache(
        &repo().join("examples/prompts-raw"),
        cache,
        &["add", "--from", &width, "ai-direct:text-width@0.1.0"],
    );
    assert!(out.status.success(), "{}", stderr(&out));
    let base64 = repo()
        .parent()
        .expect("workspace parent")
        .join("ai-direct-ir-providers/providers/base64")
        .to_string_lossy()
        .into_owned();
    let out = run_with_cache(
        &repo().join("examples/base64"),
        cache,
        &["add", "--from", &base64, "ai-direct:base64@0.1.0"],
    );
    assert!(out.status.success(), "{}", stderr(&out));
}

fn example_cache(name: &str) -> PathBuf {
    let cache = scratch(name);
    install_example_providers(&cache);
    cache
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

/// Scaffold a component project in a fresh directory and return the project
/// dir. `component` is the default target and the only one the harness hosts,
/// so it is what the assembler tests below build against.
fn scaffold(name: &str) -> PathBuf {
    scaffold_target(name, "component")
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

/// Add an include of `fragment` to the end of the starter's core module. The
/// fragments below are core-level definitions, which is where an application
/// splits its source; the component wrapper around them is the boundary.
fn include_fragment(project: &Path, relative: &str, body: &str) {
    let root = project.join("app.wat");
    let source = std::fs::read_to_string(&root).expect("read root wat");
    let close = "\n  )\n  (core instance $app";
    let at = source.find(close).expect("starter closes its core module");
    let patched = format!(
        "{}\n    ;; @include {relative}{}",
        &source[..at],
        &source[at..]
    );
    std::fs::write(&root, patched).expect("write root wat");
    let fragment = project.join(relative);
    if let Some(parent) = fragment.parent() {
        std::fs::create_dir_all(parent).expect("create fragment dir");
    }
    std::fs::write(fragment, body).expect("write fragment");
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
    let cache = scratch("examples-cache");
    install_example_providers(&cache);
    let manifests = [
        "examples/base64/host.toml",
        "examples/hello/host.toml",
        "examples/pi/host.toml",
        "examples/prompts/host.toml",
        "examples/server/host.toml",
        "examples/prompts-raw/host.toml",
        "examples/gui-hello/host.toml",
        "examples/provider-demo/host.toml",
        "examples/tcp-hello/host.toml",
    ];
    for manifest in manifests {
        let out = run_with_cache(&repo, &cache, &["check", manifest]);
        assert!(
            out.status.success(),
            "check {manifest} failed: {}",
            stderr(&out)
        );
    }
    let base64 = run_with_cache(&repo, &cache, &["run", "examples/base64/host.toml"]);
    assert!(base64.status.success(), "{}", stderr(&base64));
    assert_eq!(stdout(&base64), "Zm9vYmFy");
}

/// Connect to `addr`, retrying while the guest is still starting up. The
/// component binds the socket itself, so there is nothing to wait on but the
/// socket.
fn connect(addr: &str) -> std::net::TcpStream {
    for _ in 0..200 {
        if let Ok(stream) = std::net::TcpStream::connect(addr) {
            return stream;
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    panic!("nothing listening on {addr}");
}

/// One request against `tcp-hello`, read to end of stream. Reading to EOF is
/// the point: the response carries a `Content-Length`, so a client could stop
/// early -- this waits for the close, which only happens when the component
/// drops the accepted socket.
fn request(path: &str) -> String {
    request_sized(path, 0)
}

/// A request against the `server` example, which listens on 8124.
fn server_request(path: &str) -> String {
    request_to("127.0.0.1:8124", path, 0, None)
}

/// The same, padded with a header of `filler` bytes. The component allocates
/// one `list<u8>` per request out of its bump heap, so the padding is what
/// decides how fast a loop exhausts it -- see `tcp_hello_outlives_its_bump_heap`.
fn request_sized(path: &str, filler: usize) -> String {
    request_to("127.0.0.1:8125", path, filler, None)
}

/// One request, read to end of stream. Reading to EOF is the point: both
/// example servers send a `Content-Length`, so a client could stop early --
/// this waits for the close, which happens only when the component drops the
/// accepted socket.
fn request_to(addr: &str, path: &str, filler: usize, body: Option<&str>) -> String {
    let mut socket = connect(addr);
    socket
        .set_read_timeout(Some(std::time::Duration::from_secs(10)))
        .expect("set read timeout");
    let request = match body {
        Some(body) => format!(
            "POST {path} HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        ),
        None if filler == 0 => format!("GET {path} HTTP/1.1\r\nHost: localhost\r\n\r\n"),
        None => format!(
            "GET {path} HTTP/1.1\r\nHost: localhost\r\nX-Pad: {}\r\n\r\n",
            "x".repeat(filler)
        ),
    };
    socket.write_all(request.as_bytes()).expect("send request");
    let mut response = String::new();
    std::io::Read::read_to_string(&mut socket, &mut response).expect("read response");
    response
}

/// The WIT-derived `wasi:sockets` boundary, end to end: a component that owns
/// its own listening socket, accepts connections through `pollable.block`,
/// answers on the accepted `output-stream`, and releases all three handles
/// with `resource.drop` before the next accept.
///
/// Two requests on separate connections is what proves the drops. Each is read
/// to end of stream, which arrives only because the accepted `tcp-socket` was
/// dropped, and the second is accepted only because the loop came back around.
#[test]
fn tcp_hello_example_serves_connections_until_told_to_stop() {
    let _shared = examples_lock();
    let repo = repo();
    // Build first: `run` would rebuild too, but then build progress and the
    // listener would be racing for the same stderr.
    let built = run(&repo, &["build", "examples/tcp-hello/host.toml"]);
    assert!(built.status.success(), "build failed: {}", stderr(&built));

    let child = Command::new(air_bin())
        .args(["run", "examples/tcp-hello/host.toml"])
        .current_dir(&repo)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn air");

    let first = request("/");
    assert!(first.starts_with("HTTP/1.1 200 OK\r\n"), "{first}");
    assert!(first.ends_with("\r\n\r\nhello, air!\n"), "{first}");
    let second = request("/again");
    assert_eq!(second, first, "the loop must serve a second connection");

    // `/quit` is the only thing that ends the run.
    let last = request("/quit");
    assert!(last.ends_with("hello, air!\n"), "{last}");

    let out = child.wait_with_output().expect("wait for air");
    assert!(out.status.success(), "run failed: {}", stderr(&out));
    assert!(
        stdout(&out).contains("listening on 127.0.0.1:8125"),
        "{}",
        stdout(&out)
    );
    assert!(
        stdout(&out).contains("tcp-hello: /quit"),
        "{}",
        stdout(&out)
    );
}

/// `examples/server/` as a component: it owns its accept loop through
/// `wasi:sockets`, reads `www/` through `wasi:filesystem`, and gets its digest
/// from the locked `ai-direct:sha256` provider. Nothing here goes through a
/// host syscall -- the `net.*` layer it used to depend on no longer exists.
///
/// This is coverage the Core version never had: it was only ever `air check`ed
/// and driven by hand.
#[test]
fn server_example_serves_files_and_digests() {
    let _shared = examples_lock();
    let repo = repo();
    let cache = example_cache("server-run-cache");
    let built = run_with_cache(&repo, &cache, &["build", "examples/server/host.toml"]);
    assert!(built.status.success(), "build failed: {}", stderr(&built));

    let child = Command::new(air_bin())
        .args(["run", "examples/server/host.toml"])
        .current_dir(&repo)
        .env("XDG_CACHE_HOME", cache)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn air");

    // A file from the granted directory, with the MIME type its extension
    // implies and a Content-Length the harness never computed.
    let index = server_request("/");
    assert!(index.starts_with("HTTP/1.1 200 OK\r\n"), "{index}");
    assert!(
        index.contains("Content-Type: text/html; charset=utf-8"),
        "{index}"
    );
    assert!(index.contains("<!DOCTYPE html>"), "{index}");
    let css = server_request("/style.css");
    assert!(css.contains("Content-Type: text/css"), "{css}");

    // Two requests on separate connections prove the per-connection drops:
    // each is read to end of stream, and the second is accepted only because
    // the loop came back around.
    let again = server_request("/");
    assert_eq!(again, index, "the loop must serve a second connection");

    // Nothing outside the grant, and no way to climb out of it.
    assert!(
        server_request("/nope").starts_with("HTTP/1.1 404"),
        "missing file"
    );
    let escape = request_to("127.0.0.1:8124", "/../AGENTS.md", 0, None);
    assert!(escape.starts_with("HTTP/1.1 403"), "{escape}");

    // The digest crosses a component boundary into the vendored provider.
    let digest = request_to("127.0.0.1:8124", "/sha256", 0, Some("abc"));
    assert!(
        digest.ends_with("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"),
        "{digest}"
    );

    // Each served file opens a descriptor and a stream; if they were not
    // dropped the run would accumulate two handles per request. Enough
    // requests to make that obvious, and the heap reset has to hold too.
    for n in 0..300 {
        let response = request_to("127.0.0.1:8124", "/hello", 400, None);
        assert!(
            response.ends_with("hello, air!\n"),
            "request {n}: {response}"
        );
    }

    server_request("/quit");
    let out = child.wait_with_output().expect("wait for air");
    assert!(out.status.success(), "run failed: {}", stderr(&out));
    assert!(
        stdout(&out).contains("listening on 127.0.0.1:8124"),
        "{}",
        stdout(&out)
    );
}

/// The bump heap is the other thing a loop has to give back. `blocking-read`
/// allocates a `list<u8>` per request out of a heap that frees nothing, and
/// this example died with `realloc return: beyond end of memory` until it took
/// a mark at the top of the loop and restored it at the bottom.
///
/// The default `heap=0x8000` inside one page leaves 32KiB, so the budget is
/// counted in bytes rather than requests: 400 requests of ~450 bytes each is
/// 180KiB, five times over. Padding the request is what makes that hold no
/// matter how terse the bare request line happens to be.
#[test]
fn tcp_hello_outlives_its_bump_heap() {
    let _shared = examples_lock();
    let repo = repo();
    let built = run(&repo, &["build", "examples/tcp-hello/host.toml"]);
    assert!(built.status.success(), "build failed: {}", stderr(&built));

    let child = Command::new(air_bin())
        .args(["run", "examples/tcp-hello/host.toml"])
        .current_dir(&repo)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn air");

    for n in 0..400 {
        let response = request_sized("/", 400);
        assert!(
            response.ends_with("hello, air!\n"),
            "request {n} got: {response}"
        );
    }
    request("/quit");

    let out = child.wait_with_output().expect("wait for air");
    assert!(out.status.success(), "run failed: {}", stderr(&out));
    assert!(
        !stderr(&out).contains("beyond end of memory"),
        "{}",
        stderr(&out)
    );
}

/// `wasi:sockets` is in the linker for every component, so the grant is what
/// decides whether it answers. Without one the run stops at
/// `create-tcp-socket` with `access-denied`, which is error-code 1.
#[test]
fn a_component_reaches_the_network_only_when_granted() {
    let _shared = examples_lock();
    let repo = repo();
    let source = repo.join("examples/tcp-hello");
    let project = scratch("tcp-hello-ungranted");
    std::fs::copy(source.join("tcp-hello.wat"), project.join("tcp-hello.wat"))
        .expect("copy source");
    let manifest = std::fs::read_to_string(source.join("host.toml")).expect("read manifest");
    assert!(
        manifest.lines().any(|line| line.trim() == "network = true"),
        "the example no longer grants the network; this test would prove nothing"
    );
    let ungranted: String = manifest
        .lines()
        .filter(|line| line.trim() != "network = true")
        .map(|line| format!("{line}\n"))
        .collect();
    std::fs::write(project.join("host.toml"), &ungranted).expect("write manifest");

    let denied = run(&project, &["run", "host.toml"]);
    assert!(
        !denied.status.success(),
        "an ungranted socket must not open"
    );
    assert!(
        stderr(&denied).contains("error-code 01"),
        "{}",
        stderr(&denied)
    );

    // `--net` is the shell-side grant, and it is enough on its own.
    let child = Command::new(air_bin())
        .args(["run", "--net", "host.toml"])
        .current_dir(&project)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn air");
    let response = request("/quit");
    let out = child.wait_with_output().expect("wait for air");
    assert!(out.status.success(), "run failed: {}", stderr(&out));
    assert!(response.ends_with("hello, air!\n"), "{response}");
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
fn add_locks_a_released_provider_from_the_local_store_and_dist_namespaces_it() {
    let project = scaffold("provider-store");
    let cache = scratch("provider-store-cache");
    let package = provider_fixture("sha256");
    let package = package.to_string_lossy().into_owned();
    let added = run_with_cache(
        &project,
        &cache,
        &["add", "--from", &package, "ai-direct:sha256@0.1.0"],
    );
    assert!(added.status.success(), "{}", stderr(&added));
    let manifest = std::fs::read_to_string(project.join("host.toml")).expect("read manifest");
    assert!(
        manifest.contains("package = \"ai-direct:sha256\""),
        "{manifest}"
    );
    assert!(manifest.contains("version = \"0.1.0\""), "{manifest}");
    let lock = std::fs::read_to_string(project.join("air.lock")).expect("read lock");
    assert!(lock.contains("sha256 = \"fa27d4aeb173e"), "{lock}");

    let checked = run_with_cache(&project, &cache, &["check"]);
    assert!(checked.status.success(), "{}", stderr(&checked));
    let dist = run_with_cache(&project, &cache, &["dist"]);
    assert!(dist.status.success(), "{}", stderr(&dist));
    let bundle = project.join("dist");
    assert!(
        bundle.join("air.lock").is_file(),
        "missing release lockfile"
    );
    let release = std::fs::read_to_string(bundle.join("host.toml")).expect("read release manifest");
    assert!(
        release.contains("providers/ai-direct-sha256-0.1.0-"),
        "{release}"
    );
    let ran = Command::new(bundle.join("air"))
        .args(["run", "host.toml"])
        .current_dir(&bundle)
        .output()
        .expect("run standalone bundle");
    assert!(ran.status.success(), "{}", stderr(&ran));
}

#[test]
fn a_locked_provider_never_silently_uses_missing_or_modified_store_content() {
    let project = scaffold("provider-store-integrity");
    let cache = scratch("provider-store-integrity-cache");
    let package = provider_fixture("sha256");
    let package = package.to_string_lossy().into_owned();
    let added = run_with_cache(
        &project,
        &cache,
        &["add", "--from", &package, "ai-direct:sha256@0.1.0"],
    );
    assert!(added.status.success(), "{}", stderr(&added));
    let lock = std::fs::read_to_string(project.join("air.lock")).expect("read lock");
    let hash = lock
        .lines()
        .find_map(|line| line.strip_prefix("sha256 = \"")?.strip_suffix('"'))
        .expect("artifact hash in lock");
    let artifact = cache
        .join("air/providers")
        .join(hash)
        .join("artifacts/wasm32-wasi/sha256.component.wasm");
    std::fs::write(&artifact, "not wasm").expect("tamper cached artifact");

    let checked = run_with_cache(&project, &cache, &["check"]);
    assert!(
        !checked.status.success(),
        "an override-only lock cannot repair itself"
    );
    assert!(
        stderr(&checked).contains("missing from the local store"),
        "{}",
        stderr(&checked)
    );
}

#[test]
fn a_registry_locked_provider_is_restored_when_its_cache_entry_is_corrupt() {
    let project = scaffold("provider-registry-restore");
    let cache = scratch("provider-registry-restore-cache");
    let registry = scratch("provider-registry");
    std::fs::create_dir_all(registry.join("registry")).expect("create registry index");
    let package = provider_fixture("sha256");
    std::fs::write(
        registry.join("registry/index.toml"),
        format!(
            "[[provider]]\nname = \"ai-direct:sha256\"\nversion = \"0.1.0\"\npath = {:?}\n",
            package.to_string_lossy()
        ),
    )
    .expect("write registry index");
    let manifest = project.join("host.toml");
    let text = std::fs::read_to_string(&manifest).expect("read manifest");
    std::fs::write(
        &manifest,
        format!(
            "{text}\n[registry]\nsource = {:?}\n",
            registry.to_string_lossy()
        ),
    )
    .expect("write registry manifest");
    let added = run_with_cache(&project, &cache, &["add", "ai-direct:sha256@0.1.0"]);
    assert!(added.status.success(), "{}", stderr(&added));
    let lock = std::fs::read_to_string(project.join("air.lock")).expect("read lock");
    let hash = lock
        .lines()
        .find_map(|line| line.strip_prefix("sha256 = \"")?.strip_suffix('"'))
        .expect("artifact hash in lock");
    let artifact = cache
        .join("air/providers")
        .join(hash)
        .join("artifacts/wasm32-wasi/sha256.component.wasm");
    std::fs::write(&artifact, "corrupt").expect("corrupt cache");
    let checked = run_with_cache(&project, &cache, &["check"]);
    assert!(checked.status.success(), "{}", stderr(&checked));
}

#[test]
fn provider_mismatch_names_the_provider_and_the_missing_function() {
    let project = scratch("provider-mismatch");
    // The provider offers `shout`; the consumer wants `shout` plus a
    // `whisper` nobody exports. Read-only copies, so no examples lock.
    let demo = repo().join("examples/provider-demo");
    std::fs::copy(demo.join("provider.wat"), project.join("provider.wat"))
        .expect("copy provider source");
    let consumer =
        std::fs::read_to_string(demo.join("consumer.wat")).expect("read consumer source");
    let marker = "(export \"shout\" (func (param \"text\" string) (result string)))))";
    assert!(consumer.contains(marker), "consumer fixture changed shape");
    std::fs::write(
        project.join("consumer.wat"),
        consumer.replace(
            marker,
            "(export \"shout\" (func (param \"text\" string) (result string)))\n    (export \"whisper\" (func (param \"text\" string) (result string)))))",
        ),
    )
    .expect("write consumer source");
    std::fs::write(
        project.join("host.toml"),
        "mode = \"command\"\n\n[[providers]]\nsource = \"provider.wat\"\npath = \"provider.wasm\"\n\n[app]\nsource = \"consumer.wat\"\npath = \"consumer.wasm\"\nrun = \"wasi:cli/run\"\n",
    )
    .expect("write manifest");

    let built = run(&project, &["build"]);
    assert!(built.status.success(), "{}", stderr(&built));
    let out = run(&project, &["check"]);
    assert!(!out.status.success(), "mismatch must fail check");
    let err = stderr(&out);
    // The error attributes the fault: which entry, which interface, which
    // function — not just the linker's unnamed missing import.
    assert!(err.contains("provider.wasm"), "{err}");
    assert!(err.contains("ai-direct:demo/text"), "{err}");
    assert!(err.contains("whisper"), "{err}");
}

/// The released `ai-direct:text-width` provider, exercised on the case
/// `examples/prompts-raw/` depends on: a styled label whose column count is
/// neither its 28 bytes nor its 24 characters. ANSI CSI sequences cost no
/// columns and `◆` costs one, so the answer is 15.
#[test]
fn the_vendored_width_provider_measures_terminal_columns() {
    let project = scaffold_target("vendored-width", "component");
    let artifact = repo()
        .join("air/tests/fixtures/providers/text-width")
        .join("artifacts/wasm32-wasi/text-width.component.wasm");
    assert!(artifact.exists(), "provider fixture is missing");
    let manifest = project.join("host.toml");
    let mut text = std::fs::read_to_string(&manifest).expect("read manifest");
    text.push_str(&format!(
        "\n[[providers]]\npath = {:?}\n",
        artifact.to_string_lossy()
    ));
    std::fs::write(&manifest, text).expect("write manifest");
    write_app(
        &project,
        r#"(component
  ;; @wasi stdout
  (import "ai-direct:text-width/width@0.1.0" (instance $w
    (export "columns" (func (param "text" string) (result u32)))))
  (alias export $w "columns" (func $columns))
  (core func $columns-l
    (canon lower (func $columns) (memory $memory) (realloc $realloc)))
  (core instance $prov (export "columns" (func $columns-l)))
  (core module $main
    (import "env" "memory" (memory 1))
    (import "wasi" "get-stdout" (func $get_stdout (result i32)))
    (import "wasi" "write" (func $write (param i32 i32 i32 i32)))
    (import "provider" "columns" (func $columns (param i32 i32) (result i32)))
    ;; @data 0x100..0x200
    (data $title "\1b[1;36m◆ Project setup\1b[0m")
    (data $nl "\n")
    (func $digits (param $at i32) (param $n i32) (result i32)
      (local $end i32)
      (local.set $end
        (if (result i32) (i32.ge_u (local.get $n) (i32.const 10))
          (then (call $digits (local.get $at)
                  (i32.div_u (local.get $n) (i32.const 10))))
          (else (local.get $at))))
      (i32.store8 (local.get $end)
        (i32.add (i32.const 48) (i32.rem_u (local.get $n) (i32.const 10))))
      (i32.add (local.get $end) (i32.const 1)))
    (func (export "run") (result i32)
      (local $len i32)
      (local.set $len
        (i32.sub
          (call $digits (i32.const 0x300)
            (call $columns (global.get $title.ptr) (global.get $title.len)))
          (i32.const 0x300)))
      (call $write (call $get_stdout)
        (i32.const 0x300) (local.get $len) (i32.const 0x200))
      (call $write (call $get_stdout)
        (global.get $nl.ptr) (global.get $nl.len) (i32.const 0x200))
      (i32.load (i32.const 0x200))))
  (core instance $app (instantiate $main
    (with "env" (instance $mem))
    (with "wasi" (instance $wasi))
    (with "provider" (instance $prov))))
  (func $run (result (result)) (canon lift (core func $app "run")))
  (instance $run-i (export "run" (func $run)))
  (export "wasi:cli/run@0.2.12" (instance $run-i))
)
"#,
    );
    let ran = run(&project, &["run"]);
    assert!(ran.status.success(), "{}", stderr(&ran));
    assert_eq!(stdout(&ran), "15\n");
}

/// `prompts-raw` is a component now, so its refusal travels the whole WIT
/// boundary: `available` answers a `bool` through `ai-direct:host/term`, the
/// message goes out over `wasi:cli/stdout`, and the status comes from
/// `exit-with-code`. Driving the interactive flow needs a pty and is verified
/// by hand; refusing to start without one does not.
#[test]
fn prompts_raw_example_refuses_a_pipe() {
    let _shared = examples_lock();
    let cache = example_cache("prompts-raw-cache");
    let out = Command::new(air_bin())
        .args(["run", "examples/prompts-raw/host.toml"])
        .current_dir(repo())
        .env("XDG_CACHE_HOME", cache)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("run prompts-raw");
    assert_eq!(out.status.code(), Some(2), "{}", stderr(&out));
    assert_eq!(
        stdout(&out),
        "interactive terminal required; use examples/prompts for pipes\n"
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

/// A component that prints one named data segment, so its length is whatever
/// the harness computed rather than whatever the author last typed.
fn printer(text: &str) -> String {
    format!(
        r#"(component
  ;; @wasi stdout
  (core module $main
    (import "env" "memory" (memory 1))
    (import "wasi" "get-stdout" (func $get_stdout (result i32)))
    (import "wasi" "write" (func $write (param i32 i32 i32 i32)))
    (data $msg (i32.const 0x1000) "{text}")
    (func (export "run") (result i32)
      (call $write (call $get_stdout)
        (global.get $msg.ptr) (global.get $msg.len) (i32.const 0x200))
      (i32.load (i32.const 0x200))))
  (core instance $app (instantiate $main
    (with "env" (instance $mem))
    (with "wasi" (instance $wasi))))
  (func $run (result (result)) (canon lift (core func $app "run")))
  (instance $run-i (export "run" (func $run)))
  (export "wasi:cli/run@0.2.12" (instance $run-i))
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
        r#"(component
  ;; @wasi stdout
  (core module $main
    (import "env" "memory" (memory 1))
    (data $first (i32.const 0x1000) "0123456789")
    (data $second (i32.const 0x1005) "collides"))
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
        r#"(component
  ;; @wasi stdout
  (core module $main
    (import "env" "memory" (memory 1))
    (global $base i32 (i32.const 4096))
    (data $msg (global.get $base) "x"))
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

/// A component that calls `ai-direct:host/ui` and reports what came back.
/// `label` and `button` take a `string`, so the whole exchange is by value:
/// nothing here names a host pointer, and the canonical ABI does the copy.
fn ui_caller(text: &str) -> String {
    format!(
        r#"(component
  ;; @wasi stdout ui
  (core module $main
    (import "env" "memory" (memory 1))
    (import "wasi" "get-stdout" (func $get_stdout (result i32)))
    (import "wasi" "write" (func $write (param i32 i32 i32 i32)))
    (import "ui" "label" (func $label (param i32 i32)))
    (import "ui" "button" (func $button (param i32 i32) (result i32)))
    ;; @data 0x100..0x200
    (data $msg "{text}")
    (data $no "no\n")
    (data $yes "yes\n")
    (func (export "run") (result i32)
      (call $label (global.get $msg.ptr) (global.get $msg.len))
      (if (call $button (global.get $msg.ptr) (global.get $msg.len))
        (then (call $write (call $get_stdout)
                (global.get $yes.ptr) (global.get $yes.len) (i32.const 0x300)))
        (else (call $write (call $get_stdout)
                (global.get $no.ptr) (global.get $no.len) (i32.const 0x300))))
      (i32.const 0)))
  (core instance $app (instantiate $main
    (with "env" (instance $mem))
    (with "wasi" (instance $wasi))
    (with "ui" (instance $ui))))
  (func $run (result (result)) (canon lift (core func $app "run")))
  (instance $run-i (export "run" (func $run)))
  (export "wasi:cli/run@0.2.12" (instance $run-i))
)
"#
    )
}

/// The UI capability is a WIT interface, so any component may import it --
/// `mode = "gui"` decides who calls the entry point, not what it can reach.
/// Nothing is clicked outside a window, so `button` answers false.
#[test]
fn the_ui_interface_answers_a_component_by_value() {
    let project = scaffold_target("ui-interface", "component");
    write_app(&project, &ui_caller(r"caf\u{e9} \u{2713}"));
    let ran = run(&project, &["run"]);
    assert!(ran.status.success(), "{}", stderr(&ran));
    assert_eq!(stdout(&ran), "no\n");
}

/// The retired Core `ui.*` wrappers read `(ptr, len)` out of guest memory and
/// had to bound-check and UTF-8-validate by hand. Stated in WIT the canonical
/// ABI does it, and a `string` that is not UTF-8 never reaches the host.
#[test]
fn the_ui_interface_rejects_text_that_is_not_utf8() {
    let project = scaffold_target("ui-utf8", "component");
    // A lone continuation byte: valid in a `list<u8>`, never in a `string`.
    write_app(&project, &ui_caller(r"\80"));
    let ran = run(&project, &["run"]);
    assert!(
        !ran.status.success(),
        "invalid UTF-8 must not reach the host"
    );
    assert!(
        stderr(&ran).to_lowercase().contains("utf-8"),
        "{}",
        stderr(&ran)
    );
}

/// A GUI app is an ordinary component: `gui` is a `mode`, so the manifest
/// states no target at all and the artifact's own preamble settles it.
#[test]
fn a_gui_project_is_a_component_checked_without_a_window() {
    let project = scaffold_target("gui-scaffold", "gui");
    let manifest = std::fs::read_to_string(project.join("host.toml")).expect("read manifest");
    assert!(manifest.contains("mode = \"gui\""), "{manifest}");
    assert!(!manifest.contains("target"), "{manifest}");

    let built = run(&project, &["build"]);
    assert!(built.status.success(), "build failed: {}", stderr(&built));
    let bytes = std::fs::read(project.join("app.wasm")).expect("read artifact");
    assert_eq!(&bytes[4..6], &[0x0d, 0x00], "a GUI app must be a component");

    // Linking, granting and instantiating are the component path; only the
    // frame loop needs a display, and `check` never enters it.
    let checked = run(&project, &["check"]);
    assert!(
        checked.status.success(),
        "check failed: {}",
        stderr(&checked)
    );
    let report = stdout(&checked);
    assert!(report.contains("run `frame`: signature ok"), "{report}");
    assert!(report.contains("all imports satisfied"), "{report}");
}

/// The harness hosts components and nothing else. A Core module can still be
/// an application in the browser, but that is a decision the manifest has to
/// state -- and the error has to say what to do about it, because the answer
/// is a build step rather than a manifest key.
#[test]
fn a_core_module_has_no_host() {
    let project = scaffold("core-no-host");
    write_app(&project, "(module (func (export \"run\")))\n");
    // Nothing declared, so nothing but the artifact can answer the question.
    let manifest = project.join("host.toml");
    let declared = std::fs::read_to_string(&manifest).expect("read manifest");
    let without: String = declared
        .lines()
        .filter(|line| !line.starts_with("target"))
        .map(|line| format!("{line}\n"))
        .collect();
    std::fs::write(&manifest, without).expect("write manifest");
    let out = run(&project, &["check"]);
    assert!(!out.status.success(), "a Core app must be rejected");
    let message = stderr(&out);
    assert!(message.contains("Core WASM module"), "{message}");
    assert!(message.contains("target = \"browser\""), "{message}");
    assert!(message.contains("wasm-tools component new"), "{message}");
}

/// A frame loop needs `ai-direct:host/ui`, which only a component can import.
/// A manifest that pairs `mode = "gui"` with a Core target says so at load,
/// not at an unresolved import.
#[test]
fn a_gui_manifest_needs_a_component() {
    let project = scaffold_target("gui-core-module", "gui");
    write_app(&project, "(module (func (export \"frame\")))\n");
    let manifest = project.join("host.toml");
    let text = std::fs::read_to_string(&manifest).expect("read manifest");
    std::fs::write(&manifest, format!("target = \"browser\"\n{text}")).expect("write manifest");
    let out = run(&project, &["check"]);
    assert!(!out.status.success(), "a Core GUI app must be rejected");
    assert!(
        stderr(&out).contains("needs a component"),
        "{}",
        stderr(&out)
    );
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
fn a_distribution_carries_its_providers_and_its_grants() {
    let _shared = examples_lock();
    let repo = repo();
    let cache = example_cache("server-dist-cache");
    let built = run_with_cache(&repo, &cache, &["build", "examples/server/host.toml"]);
    assert!(built.status.success(), "build failed: {}", stderr(&built));
    let out = run_with_cache(&repo, &cache, &["dist", "examples/server/host.toml"]);
    assert!(out.status.success(), "dist failed: {}", stderr(&out));
    let dist = repo.join("examples/server/dist");

    // The artifact imports the provider's interface rather than containing it,
    // so a distribution without the component cannot instantiate.
    assert!(
        dist.join("providers/ai-direct-sha256-0.1.0-fa27d4aeb173.wasm")
            .is_file(),
        "dist did not bundle the provider component"
    );
    let manifest = std::fs::read_to_string(dist.join("host.toml")).expect("read dist manifest");
    // A grant belongs to the application, not to the shell that packaged it.
    assert!(manifest.contains("network = true"), "{manifest}");
    assert!(
        manifest.contains("providers/ai-direct-sha256-0.1.0-fa27d4aeb173.wasm"),
        "{manifest}"
    );

    // The real proof is that the copy runs on its own.
    let child = Command::new(dist.join("air"))
        .args(["run", "host.toml"])
        .current_dir(&dist)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn packaged air");
    let index = server_request("/");
    assert!(index.contains("<!DOCTYPE html>"), "{index}");
    let digest = request_to("127.0.0.1:8124", "/sha256", 0, Some("abc"));
    assert!(
        digest.ends_with("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"),
        "{digest}"
    );
    server_request("/quit");
    let done = child.wait_with_output().expect("wait for packaged air");
    assert!(done.status.success(), "{}", stderr(&done));
    std::fs::remove_dir_all(&dist).ok();
}

/// A `root` that cannot be copied into the bundle must not stop the packaging.
/// `examples/sha256sum/` grants `root = "../.."` -- the whole repository -- as
/// a development convenience; that is not a directory to ship, and resolving
/// it used to fail with an unexplained "root has no directory name".
#[test]
fn a_root_that_cannot_travel_is_dropped_with_a_note() {
    let _shared = examples_lock();
    let repo = repo();
    let cache = example_cache("sha256-dist-cache");
    let out = run_with_cache(&repo, &cache, &["dist", "examples/sha256sum/host.toml"]);
    assert!(out.status.success(), "dist failed: {}", stderr(&out));
    assert!(stderr(&out).contains("cannot travel"), "{}", stderr(&out));
    assert!(stderr(&out).contains("--dir"), "{}", stderr(&out));

    let dist = repo.join("examples/sha256sum/dist");
    let manifest = std::fs::read_to_string(dist.join("host.toml")).expect("read dist manifest");
    // Dropping the grant narrows what the packaged app reaches, and the whole
    // repository is emphatically not in the bundle.
    assert!(!manifest.contains("root"), "{manifest}");
    assert!(
        !dist.join("ai-direct-ir").exists(),
        "the repo got copied in"
    );
    assert!(
        dist.join("providers/ai-direct-sha256-0.1.0-fa27d4aeb173.wasm")
            .is_file(),
        "provider missing"
    );

    // It still runs, granted from the shell the way the note says.
    let file = dist.join("subject.txt");
    std::fs::write(&file, "abc").expect("write subject");
    let out = Command::new(dist.join("air"))
        .args(["run", "--dir", ".", "host.toml", "subject.txt"])
        .current_dir(&dist)
        .output()
        .expect("run packaged air");
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(
        stdout(&out)
            .starts_with("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"),
        "{}",
        stdout(&out)
    );
    std::fs::remove_dir_all(&dist).ok();
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

/// The harness's own interfaces are generated from WIT like a WASI one.
/// `air/wit/ai-direct-host/host.wit` is the file `component.rs` implements, so
/// `;; @wasi term` cannot describe a signature the host does not offer -- the
/// 34 hand-written lines this replaces in `examples/prompts-raw` could.
#[test]
fn the_term_capability_is_generated_from_wit() {
    let project = scaffold_target("term-generated", "component");
    write_app(&project, &term_caller());
    let checked = run(&project, &["check"]);
    assert!(checked.status.success(), "{}", stderr(&checked));
    let ran = run(&project, &["run"]);
    assert!(ran.status.success(), "{}", stderr(&ran));
    // Same answer as the hand-declared boundary below: tests have no terminal.
    assert_eq!(stdout(&ran), "terminal: no\n");
}

/// A name the WIT does not declare is an error at the directive, not an
/// unresolved import discovered inside the component linker.
#[test]
fn the_term_capability_rejects_a_name_it_does_not_declare() {
    let project = scaffold_target("term-unknown", "component");
    write_app(
        &project,
        &term_caller().replace(
            r#"(import "term" "available" (func $available (result i32)))"#,
            r#"(import "term" "available" (func $available (result i32)))
    (import "term" "resize" (func $resize (param i32 i32)))"#,
        ),
    );
    let built = run(&project, &["build"]);
    assert!(!built.status.success(), "{}", stdout(&built));
    let message = stderr(&built);
    assert!(message.contains("\"resize\""), "{message}");
    assert!(message.contains("ai-direct:host"), "{message}");
}

/// `ui` and `term` are one WIT package, so asking for one must not drag in the
/// other: a component that draws imports no terminal.
#[test]
fn one_host_capability_does_not_import_the_other() {
    let project = scaffold_target("host-one", "component");
    write_app(&project, &ui_caller("hi"));
    assert!(run(&project, &["build"]).status.success());
    let bytes = std::fs::read(project.join("app.wasm")).expect("read artifact");
    let text = String::from_utf8_lossy(&bytes);
    assert!(text.contains("ai-direct:host/ui"), "ui must be imported");
    assert!(
        !text.contains("ai-direct:host/term"),
        "a drawing component imported the terminal"
    );
}

/// A component that asks for a capability and imports nothing from it is a
/// stale directive, and saying so beats emitting an instance nothing uses.
#[test]
fn a_host_capability_with_no_imports_is_an_error() {
    let project = scaffold_target("term-unused", "component");
    set_wasi_directive(&project, "stdout term");
    let built = run(&project, &["build"]);
    assert!(!built.status.success(), "{}", stdout(&built));
    assert!(stderr(&built).contains("\"term\""), "{}", stderr(&built));
}

/// Reads `available` through the generated `$term` instance and reports it.
fn term_caller() -> String {
    r#"(component
  ;; @wasi stdout term
  (core module $main
    (import "env" "memory" (memory 1))
    (import "wasi" "get-stdout" (func $get_stdout (result i32)))
    (import "wasi" "write" (func $write (param i32 i32 i32 i32)))
    ;; `available` answers a `bool`, which lowers to an `i32` that is 0 or 1.
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
    (with "term" (instance $term))))
  (func $run (result (result)) (canon lift (core func $app "run")))
  (instance $run-i (export "run" (func $run)))
  (export "wasi:cli/run@0.2.12" (instance $run-i))
)
"#
    .to_string()
}

/// The directive is a shorthand, never a gate: the same interface declared by
/// hand still links, which is what a component consuming an interface `air`
/// does *not* implement has to do.
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
  ;; `available` answers a `bool`, which lowers to an `i32` that is 0 or 1 --
  ;; not a status code the caller has to know the convention for.
  (import "ai-direct:host/term" (instance $term
    (export "available" (func (result bool)))))
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

/// Give the scaffolded component one filesystem call, so `;; @wasi filesystem`
/// has an `(import "fs" ...)` line to generate a boundary for.
fn use_filesystem(project: &Path) {
    let root = project.join("app.wat");
    let source = std::fs::read_to_string(&root).expect("read root wat");
    let rewritten = source
        .replace(
            "    (import \"wasi\" \"write\" (func $write (param i32 i32 i32 i32)))",
            "    (import \"wasi\" \"write\" (func $write (param i32 i32 i32 i32)))\n\
             \x20   (import \"fs\" \"get-directories\" (func $get-dirs (param i32)))",
        )
        .replace(
            "      (i32.load (i32.const 0x200)))",
            "      (call $get-dirs (i32.const 0x300))\n\
             \x20     (i32.load (i32.const 0x200)))",
        )
        .replace(
            "    (with \"wasi\" (instance $wasi))))",
            "    (with \"wasi\" (instance $wasi))\n\
             \x20   (with \"fs\" (instance $fs))))",
        );
    assert!(rewritten.contains("$get-dirs"), "fs import not injected");
    assert!(rewritten.contains("(with \"fs\""), "fs instance not wired");
    std::fs::write(&root, rewritten).expect("write root wat");
}

/// `filesystem` derives the whole `wasi:filesystem` boundary from the
/// vendored WIT: the artifact imports both interfaces with nothing
/// hand-transcribed, and the component still links and runs.
///
/// It derives the *extent* of that boundary too, from the application's own
/// `(import "fs" ...)` lines. Import and export names survive into the
/// artifact as literal strings, so the bytes are the evidence that a program
/// naming one function does not carry the WIT's other 28.
#[test]
fn the_wasi_directive_derives_filesystem_from_wit() {
    let project = scaffold_target("wasi-filesystem", "component");
    set_wasi_directive(&project, "stdout filesystem");
    use_filesystem(&project);

    let built = run(&project, &["build"]);
    assert!(built.status.success(), "build failed: {}", stderr(&built));
    let bytes = std::fs::read(project.join("app.wasm")).expect("read artifact");
    let names = String::from_utf8_lossy(&bytes).into_owned();
    assert!(
        names.contains("wasi:filesystem/types@"),
        "filesystem was requested"
    );
    assert!(
        names.contains("wasi:filesystem/preopens@"),
        "preopens comes with filesystem"
    );
    assert!(names.contains("get-directories"), "the program calls it");
    for unasked in [
        "descriptor.open-at",
        "read-via-stream",
        "write-via-stream",
        "metadata-hash",
        "cross-device",
    ] {
        assert!(
            !names.contains(unasked),
            "`{unasked}` was never imported from `fs`"
        );
    }

    let ran = run(&project, &["run"]);
    assert!(ran.status.success(), "run failed: {}", stderr(&ran));
    assert_eq!(stdout(&ran), "hello from app\n");
}

/// The `(import "fs" ...)` line may be anywhere in the expanded source, which
/// is why the boundary is generated after expansion rather than in place: here
/// the whole application module moves into an included fragment, below the
/// directive that has to account for it.
#[test]
fn an_fs_import_inside_an_include_still_drives_the_boundary() {
    let project = scaffold_target("wasi-filesystem-include", "component");
    set_wasi_directive(&project, "stdout filesystem");
    use_filesystem(&project);

    let root = project.join("app.wat");
    let source = std::fs::read_to_string(&root).expect("read root wat");
    let marker = "  ;; --- application logic";
    let (head, rest) = source.split_once(marker).expect("starter layout");
    let (module, tail) = rest
        .split_once("\n  (core instance $app")
        .expect("starter layout");
    std::fs::create_dir_all(project.join("src")).expect("create src");
    std::fs::write(project.join("src/main.wat"), format!("{marker}{module}\n"))
        .expect("write fragment");
    std::fs::write(
        &root,
        format!("{head}  ;; @include src/main.wat\n  (core instance $app{tail}"),
    )
    .expect("write root wat");

    let built = run(&project, &["build"]);
    assert!(built.status.success(), "build failed: {}", stderr(&built));
    let bytes = std::fs::read(project.join("app.wasm")).expect("read artifact");
    let names = String::from_utf8_lossy(&bytes).into_owned();
    assert!(names.contains("get-directories"), "the fragment calls it");
    assert!(!names.contains("open-at"), "nothing imported it");

    let ran = run(&project, &["run"]);
    assert!(ran.status.success(), "run failed: {}", stderr(&ran));
    assert_eq!(stdout(&ran), "hello from app\n");
}

/// The boundary is the answer to a question the modules ask. Asking for
/// `filesystem` and then importing nothing from `"fs"` is a mistake worth
/// naming, not an empty instance to puzzle over later.
#[test]
fn filesystem_without_an_fs_import_is_rejected() {
    let project = scaffold_target("wasi-filesystem-unused", "component");
    set_wasi_directive(&project, "stdout filesystem");

    let built = run(&project, &["build"]);
    assert!(!built.status.success(), "an unused boundary must not build");
    let message = stderr(&built);
    assert!(message.contains("app.wat"), "{message}");
    assert!(message.contains("no module imports"), "{message}");
}

/// A misspelled `fs` import is a typo in the application, and the WIT knows
/// it: say so at build time rather than failing to link.
#[test]
fn an_unknown_fs_import_is_rejected() {
    let project = scaffold_target("wasi-filesystem-typo", "component");
    set_wasi_directive(&project, "stdout filesystem");
    use_filesystem(&project);
    let root = project.join("app.wat");
    let source = std::fs::read_to_string(&root).expect("read root wat");
    std::fs::write(&root, source.replace("get-directories", "get-directoriez"))
        .expect("write root wat");

    let built = run(&project, &["build"]);
    assert!(!built.status.success(), "a typo must not build");
    let message = stderr(&built);
    assert!(message.contains("get-directoriez"), "{message}");
    assert!(message.contains("wasi:filesystem"), "{message}");
}

/// A drop is only offered for a resource the boundary declares. Asking to
/// release one it never handed out would otherwise surface as an unresolved
/// core import, well away from the line that asked.
#[test]
fn dropping_a_resource_the_boundary_never_declared_is_rejected() {
    let project = scaffold_target("wasi-drop-undeclared", "component");
    set_wasi_directive(&project, "stdout");
    let root = project.join("app.wat");
    let source = std::fs::read_to_string(&root).expect("read root wat");
    std::fs::write(
        &root,
        source.replace(
            "    (import \"wasi\" \"write\" (func $write (param i32 i32 i32 i32)))",
            "    (import \"wasi\" \"write\" (func $write (param i32 i32 i32 i32)))\n\
             \x20   (import \"wasi\" \"input-stream.drop\" (func $drop-in (param i32)))",
        ),
    )
    .expect("write root wat");

    let built = run(&project, &["build"]);
    assert!(!built.status.success(), "an undeclared drop must not build");
    let message = stderr(&built);
    assert!(message.contains("input-stream.drop"), "{message}");
    // `stdout` declares only the output stream, and the error says so.
    assert!(message.contains("`output-stream`"), "{message}");
    assert!(message.contains("app.wat"), "{message}");
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
    let cache = example_cache("sha256-run-cache");

    // "abc" is the FIPS 180-4 vector; the file holds exactly those three bytes.
    let fixture = project.join("abc.txt");
    std::fs::write(&fixture, b"abc").expect("write fixture");

    // The manifest grants the repository, and its paths are project-relative,
    // so the argument is named from the repository root wherever `air` is run.
    let arg = "examples/sha256sum/abc.txt";
    let out = run_with_cache(&project, &cache, &["run", "host.toml", arg]);
    assert!(out.status.success(), "run failed: {}", stderr(&out));
    assert_eq!(
        stdout(&out),
        format!("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad  {arg}\n")
    );

    let _ = std::fs::remove_file(&fixture);
}

/// Arguments reach the guest, and the app distinguishes its exit codes.
#[test]
fn sha256sum_example_reports_usage_and_errors() {
    let _guard = examples_lock();
    let project = repo().join("examples/sha256sum");
    let cache = example_cache("sha256-errors-cache");

    let helped = run_with_cache(&project, &cache, &["run", "host.toml", "--help"]);
    assert!(helped.status.success(), "{}", stderr(&helped));
    assert!(stdout(&helped).contains("usage:"), "{}", stdout(&helped));

    // No argument at all: argv reached the guest and it saw only argv[0].
    let bare = run_with_cache(&project, &cache, &["run", "host.toml"]);
    assert_eq!(bare.status.code(), Some(1), "{}", stderr(&bare));
    assert!(
        stderr(&bare).contains("expected one file"),
        "{}",
        stderr(&bare)
    );

    let missing = run_with_cache(&project, &cache, &["run", "host.toml", "nope.txt"]);
    assert_eq!(missing.status.code(), Some(2), "{}", stderr(&missing));
}

/// `--dir` grants a directory the manifest did not, which is what makes a tool
/// usable on files outside its own project. WASI has no global root, so
/// without a grant nothing outside the manifest's `root` is readable.
#[test]
fn dir_grants_a_directory_to_the_guest() {
    let _guard = examples_lock();
    let project = repo().join("examples/sha256sum");
    let cache = example_cache("sha256-dir-cache");
    let manifest = project.join("host.toml");
    let manifest = manifest.to_str().expect("utf-8 path");

    // The manifest grants the repository, so reach for something outside it.
    // Nothing is readable that was not granted, and the app says so.
    let denied = run_with_cache(&repo(), &cache, &["run", manifest, "/etc/hostname"]);
    assert_eq!(denied.status.code(), Some(2), "{}", stderr(&denied));
    let message = stderr(&denied);
    assert!(message.contains("cannot open"), "{message}");
    assert!(message.contains("--dir"), "{message}");

    // `--dir /` grants everything, and an absolute path then works as written.
    let granted = run_with_cache(
        &repo(),
        &cache,
        &["run", "--dir", "/", manifest, "/etc/hostname"],
    );
    assert!(granted.status.success(), "{}", stderr(&granted));
    let digest = std::process::Command::new("sha256sum")
        .arg("/etc/hostname")
        .output()
        .expect("sha256sum");
    let expected = String::from_utf8_lossy(&digest.stdout).into_owned();
    let expected_hash = expected.split_whitespace().next().expect("hash");
    assert!(
        stdout(&granted).starts_with(expected_hash),
        "{}",
        stdout(&granted)
    );
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

/// A component gets no ambient filesystem. `[[dirs]]` grants one, read-only
/// unless it asks for writes, which is what a stateful app -- a database, a
/// cache, a log -- needs before it can keep anything.
#[test]
fn a_manifest_dir_is_writable_only_when_it_says_so() {
    let dir = scratch("manifest-dirs");
    let project = dir.join("app");
    std::fs::create_dir_all(&project).expect("create project");

    // A component that creates `out.txt` in the first granted directory.
    let source = repo().join("air/tests/fixtures/write-file.wat");
    std::fs::copy(&source, project.join("app.wat")).expect("copy fixture");

    let manifest = |write: &str| {
        format!(
            "mode = \"command\"\n\
             [[dirs]]\n\
             path = \"data\"\n{write}\
             [app]\n\
             source = \"app.wat\"\n\
             path = \"app.wasm\"\n\
             run = \"wasi:cli/run\"\n"
        )
    };

    // Read-only: the grant exists, the write does not happen.
    std::fs::create_dir_all(project.join("data")).expect("create data");
    std::fs::write(project.join("ro.toml"), manifest("")).expect("write manifest");
    let denied = run(&project, &["run", "ro.toml"]);
    assert!(!denied.status.success(), "a read-only grant must refuse");
    assert!(!project.join("data/out.txt").exists(), "file was created");

    // `write = true`: the file appears.
    std::fs::write(project.join("rw.toml"), manifest("write = true\n")).expect("write manifest");
    let allowed = run(&project, &["run", "rw.toml"]);
    assert!(allowed.status.success(), "run failed: {}", stderr(&allowed));
    let written = std::fs::read_to_string(project.join("data/out.txt")).expect("read output");
    assert_eq!(written, "written by a component\n");
}

/// A manifest path is project-relative, so a granted directory is the same one
/// wherever `air` was launched from, and a writable one is created on demand.
#[test]
fn a_writable_manifest_dir_is_created_and_project_relative() {
    let dir = scratch("manifest-dirs-relative");
    let project = dir.join("app");
    std::fs::create_dir_all(&project).expect("create project");
    std::fs::copy(
        repo().join("air/tests/fixtures/write-file.wat"),
        project.join("app.wat"),
    )
    .expect("copy fixture");
    std::fs::write(
        project.join("host.toml"),
        "mode = \"command\"\n\
         [[dirs]]\n\
         path = \"data\"\n\
         write = true\n\
         [app]\n\
         source = \"app.wat\"\n\
         path = \"app.wasm\"\n\
         run = \"wasi:cli/run\"\n",
    )
    .expect("write manifest");

    // `data/` does not exist yet, and the run happens from somewhere else.
    assert!(!project.join("data").exists());
    let manifest = project.join("host.toml");
    let out = run(&dir, &["run", manifest.to_str().expect("utf-8 path")]);
    assert!(out.status.success(), "run failed: {}", stderr(&out));
    assert!(
        project.join("data/out.txt").exists(),
        "the directory resolved against the shell, not the manifest"
    );
}
