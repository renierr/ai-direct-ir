//! `air new` and `air init` -- project scaffolding and its starter sources.

use wasmtime::{Engine, Result};

use crate::manifest::Manifest;

/// What `air new` offers to scaffold. This is not `manifest::Target`: `Gui` is
/// a component whose manifest says `mode = "gui"`, so the two would-be enums
/// answer different questions -- what to write out, and what to link against.
#[derive(PartialEq, Eq)]
enum Kind {
    Component,
    Browser,
    Gui,
}

use crate::fail;

use super::build::build_wat;

/// Scaffold a full project dir plus AI-facing intent, architecture, and test docs.
/// Templates are baked into the binary (include_str!), so a fresh project
/// carries harness instructions and rules with it. Never overwrites.
pub fn cmd_new(engine: &Engine, name: &str) -> Result<()> {
    if name.is_empty()
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    {
        return fail(format!("bad project name `{name}`: use [A-Za-z0-9_-]"));
    }
    let target = prompt_target()?;
    let dir = std::path::Path::new(name);
    if dir.exists() {
        let empty = std::fs::read_dir(dir)
            .map(|mut d| d.next().is_none())
            .unwrap_or(false);
        if !empty {
            return fail(format!("`{name}` exists and is not empty, refusing"));
        }
    } else {
        std::fs::create_dir_all(dir)?;
    }
    let wat = dir.join(format!("{name}.wat"));
    let toml = dir.join("host.toml");
    let readme = dir.join("README.md");
    let agents = dir.join("AGENTS.md");
    let gitignore = dir.join(".gitignore");
    let index = dir.join("index.html");
    let web_host = dir.join("web-host.js");
    let docs = dir.join("docs");
    let src = dir.join("src");
    let state = src.join("state.wat");
    let src_readme = src.join("README.md");
    let skills = dir.join(".agents").join("skills").join("ai-direct-ir");
    let skill = skills.join("SKILL.md");
    let spec = docs.join("01-spec.md");
    let architecture = docs.join("02-architecture.md");
    let verification = docs.join("03-verification.md");
    let mut files = vec![
        &wat,
        &state,
        &toml,
        &readme,
        &agents,
        &gitignore,
        &src_readme,
        &skill,
    ];
    if target == Kind::Browser {
        files.extend([&index, &web_host]);
    }
    for p in files {
        if p.exists() {
            return fail(format!("`{}` exists, refusing to overwrite", p.display()));
        }
    }
    for p in [&spec, &architecture, &verification] {
        if p.exists() {
            return fail(format!("`{}` exists, refusing to overwrite", p.display()));
        }
    }
    let (starter, manifest) = match target {
        Kind::Browser => browser_starter(name),
        Kind::Gui => gui_starter(name),
        Kind::Component => component_starter(name),
    };
    std::fs::create_dir_all(&src)?;
    std::fs::write(
        &state,
        ";; Application state and state-transition helpers belong here.\n",
    )?;
    std::fs::write(&wat, starter)?;
    std::fs::write(&toml, manifest)?;
    std::fs::write(
        &gitignore,
        include_str!("../../templates/project-gitignore"),
    )?;
    std::fs::write(
        &readme,
        project_doc(
            include_str!("../../templates/project-readme.md"),
            name,
            &target,
        ),
    )?;
    std::fs::create_dir_all(&docs)?;
    std::fs::write(
        &src_readme,
        project_doc(
            include_str!("../../templates/project-src-readme.md"),
            name,
            &target,
        ),
    )?;
    std::fs::create_dir_all(&skills)?;
    std::fs::write(
        &skill,
        project_doc(
            include_str!("../../templates/project-skill.md"),
            name,
            &target,
        ),
    )?;
    std::fs::write(
        &spec,
        project_doc(
            include_str!("../../templates/project-spec.md"),
            name,
            &target,
        ),
    )?;
    std::fs::write(
        &architecture,
        project_doc(
            include_str!("../../templates/project-architecture.md"),
            name,
            &target,
        ),
    )?;
    std::fs::write(
        &verification,
        project_doc(
            include_str!("../../templates/project-verification.md"),
            name,
            &target,
        ),
    )?;
    std::fs::write(
        &agents,
        project_doc(
            include_str!("../../templates/project-agents.md"),
            name,
            &target,
        ),
    )?;
    if target == Kind::Browser {
        std::fs::write(&index, include_str!("../../templates/browser-index.html"))?;
        std::fs::write(
            &web_host,
            include_str!("../../templates/browser-host.js").replace("__APPNAME__", name),
        )?;
    }
    let manifest: Manifest = crate::manifest::load(toml.to_str().unwrap())?;
    build_wat(engine, toml.to_str().unwrap(), &manifest)?;
    let extra = if target == Kind::Browser {
        "\n  index.html\n  web-host.js"
    } else {
        ""
    };
    println!(
        "created {name}/:\n  {name}.wat\n  {name}.wasm\n  host.toml\n  README.md\n  AGENTS.md\n  docs/\n  src/\n  .agents/skills/ai-direct-ir/\n  .gitignore{extra}\n\
         next:\n  cd {name} && air check{}",
        if target == Kind::Browser {
            " && air serve"
        } else {
            " && air run"
        }
    );
    Ok(())
}

/// Render one application-focused document for the selected target; generated
/// projects should not carry irrelevant instructions for the other runtime.
fn project_doc(template: &str, name: &str, target: &Kind) -> String {
    let (target_name, run_command, workflow, files, contract, verify, agent_contract) = if *target
        == Kind::Browser
    {
        (
            "browser",
            "air serve",
            "`air serve` hosts this directory at a localhost URL with the required\nWASM MIME type. Open that URL in a browser. `air run` is not used for\nbrowser projects. `air dist` contains `index.html`, `web-host.js`, and the\ncompiled application; deploy that directory to any static web host.",
            "| `index.html` | The page containing the application canvas. |\n| `web-host.js` | Trusted browser runtime that implements the `web.*` imports. |",
            "The module exports `start()` (the `[app].run` entry). It may import only the\ndeclared `web.*` functions implemented in `web-host.js`: Canvas dimensions,\n`clear`, `fill_rect`, keyboard state, pointer coordinates, and frame scheduling.\nIf it imports `request_frame()`, it must export `frame()`. `web-host.js` owns\nbrowser events and drawing effects; WAT owns application state and behavior.",
            "Use `air serve` and test the result in a browser",
            "- `web-host.js` is trusted application runtime, not generated glue to discard.\n  Keep its imports and the WAT imports in lockstep.\n- Do not import WASI or any host interface: browser validation rejects\n  everything but the declared `web.*` functions.\n- Keep rendering explicit through `web.*`; do not add arbitrary JavaScript\n  evaluation or DOM object handles as shortcuts.",
        )
    } else if *target == Kind::Gui {
        (
            "native GUI component",
            "air run",
            "`air run` opens the native egui window and calls the configured entry once per UI frame. `air check` links and instantiates without opening a window. `air dist` contains the executable, manifest, and compiled application.",
            "",
            "The component exports a zero-argument frame function. It imports `ai-direct:host/ui` and may import any WASI 0.2 interface or `[[providers]]` component. WAT owns state; the host renders the controls using egui.",
            "Run `air run`, interact with the window, and confirm expected state changes",
            "- `label: func(text: string)` and `button: func(text: string) -> bool` are the host's UI interface, not a limit on application dependencies. Add component dependencies through `[[providers]]`.
- The entry runs once per UI frame. Button clicks are returned on the following frame; retain application state in WAT globals or memory.
- `air check` links the complete declared graph. An unresolved import is an integration error, not a reason to add an application-specific harness API.",
        )
    } else {
        (
            "component",
            "air run",
            "`air run` links the component, applies the manifest's grants, and calls\nthe entry point. `air dist` contains the `air` executable, a rewritten local\n`host.toml`, the component, every declared provider, and any configured\n`root` data directory.",
            "",
            "The component exports `wasi:cli/run` (the `[app].run` entry). `;; @wasi`\ngenerates the boundary from the capabilities it names -- WASI 0.2 plus the\nharness's own `term` and `ui` -- narrowed by the application's own imports. Nothing is reachable unless the manifest grants it:\ndirectories through `root`/`[[dirs]]`, sockets through `network = true`.",
            "Run `air run` and exercise the expected CLI behavior",
            "- Never hand-write the WASI boundary. `;; @wasi <capabilities>` generates the\n  imports, the shared memory and the canonical ABI lowering.\n- Depend on other components through `[[providers]]`, wired at link time. A\n  prebuilt Core module is not a dependency: lift it with `wasm-tools component\n  new` first.\n- `air check` links and instantiates the complete declared graph. An unresolved\n  import is an integration error, not a reason to add a harness API.",
        )
    };
    template
        .replace("__APPNAME__", name)
        .replace("__TARGET_NAME__", target_name)
        .replace("__RUN_COMMAND__", run_command)
        .replace("__TARGET_WORKFLOW__", workflow)
        .replace("__TARGET_FILES__", files)
        .replace("__TARGET_CONTRACT__", contract)
        .replace("__VERIFY_ACTION__", verify)
        .replace("__TARGET_AGENT_CONTRACT__", agent_contract)
}

fn prompt_target() -> Result<Kind> {
    use std::io::{self, IsTerminal, Write};
    if io::stdin().is_terminal() && io::stdout().is_terminal() {
        return select_target();
    }
    print!("target [component/browser/gui] (component): ");
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    match input.trim() {
        "" | "component" => Ok(Kind::Component),
        "browser" => Ok(Kind::Browser),
        "gui" => Ok(Kind::Gui),
        other => fail(format!(
            "unknown target `{other}`: choose component, browser, or gui"
        )),
    }
}

/// A compact selector keeps interactive `new` discoverable without giving up
/// the line-input fallback needed by piped scripts and CI.
fn select_target() -> Result<Kind> {
    use crossterm::{
        cursor::{MoveToColumn, MoveUp},
        event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
        execute,
        style::{Attribute, Print, SetAttribute},
        terminal::{disable_raw_mode, enable_raw_mode},
    };
    use std::io::{self, Write};

    struct RawMode;
    impl Drop for RawMode {
        fn drop(&mut self) {
            let _ = disable_raw_mode();
        }
    }

    fn draw(selected: usize) -> std::io::Result<()> {
        let mut out = io::stdout();
        execute!(
            out,
            MoveToColumn(0),
            Print("Create target (Up/Down, Enter):\r\n")
        )?;
        for (index, (name, description)) in [
            ("Component", "WASM component on WASI 0.2"),
            ("Browser", "Canvas application served to a web browser"),
            ("GUI", "Native egui desktop application"),
        ]
        .iter()
        .enumerate()
        {
            let marker = if index == selected { ">" } else { " " };
            if index == selected {
                execute!(out, SetAttribute(Attribute::Bold))?;
            }
            execute!(out, Print(format!(" {marker} {name:<9} {description}\r\n")))?;
            if index == selected {
                execute!(out, SetAttribute(Attribute::Reset))?;
            }
        }
        execute!(out, Print("\r\n"))?;
        out.flush()
    }

    enable_raw_mode().map_err(|e| wasmtime::Error::msg(format!("terminal raw mode: {e}")))?;
    let _raw = RawMode;
    let mut selected = 0;
    draw(selected).map_err(|e| wasmtime::Error::msg(format!("terminal draw: {e}")))?;
    loop {
        let event =
            event::read().map_err(|e| wasmtime::Error::msg(format!("terminal read: {e}")))?;
        let Event::Key(key) = event else { continue };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                selected = selected.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') | KeyCode::Tab => {
                selected = (selected + 1).min(2);
            }
            KeyCode::Enter => {
                println!();
                return Ok(match selected {
                    0 => Kind::Component,
                    1 => Kind::Browser,
                    _ => Kind::Gui,
                });
            }
            KeyCode::Esc => {
                println!();
                return fail("project creation cancelled".into());
            }
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                println!();
                return fail("project creation cancelled".into());
            }
            _ => continue,
        }
        let mut out = io::stdout();
        execute!(out, MoveUp(5))
            .map_err(|e| wasmtime::Error::msg(format!("terminal redraw: {e}")))?;
        draw(selected).map_err(|e| wasmtime::Error::msg(format!("terminal draw: {e}")))?;
    }
}

fn browser_starter(name: &str) -> (String, String) {
    let starter = format!(
        ";; {name}.wat -- Canvas app hosted by web-host.js.\n\
         ;; Build: air build\n\
         ;; Check: air check\n\
         ;; Run: serve this directory and open index.html.\n\
         ;; web.* is the browser ABI: keep app state in WASM and drawing explicit.\n\
         \n\
         (module\n\
          \x20 (import \"web\" \"clear\" (func $clear (param i32 i32 i32 i32)))\n\
          \x20 (import \"web\" \"fill_rect\" (func $fill_rect (param i32 i32 i32 i32 i32 i32 i32 i32)))\n\
          \x20 ;; @include src/state.wat\n\
          \x20 (func (export \"start\")\n\
         \x20   (call $clear (i32.const 20) (i32.const 24) (i32.const 35) (i32.const 255))\n\
         \x20   (call $fill_rect (i32.const 48) (i32.const 48) (i32.const 320) (i32.const 160)\n\
         \x20     (i32.const 67) (i32.const 151) (i32.const 255) (i32.const 255)))\n\
         )\n"
    );
    let manifest = format!(
        "# {name}: browser Canvas app.\n\
         target = \"browser\"\n\
         mode = \"command\"\n\
         \n\
         [app]\n\
         source = \"{name}.wat\"\n\
         path = \"{name}.wasm\"\n\
         run = \"start\"\n"
    );
    (starter, manifest)
}

/// A WASI 0.2 command component, authored as component WAT. `air` assembles
/// it in-process: the component path needs no bindings generator and no
/// language toolchain, exactly like the Core path.
fn component_starter(name: &str) -> (String, String) {
    let starter = include_str!("../../templates/component-starter.wat").replace("__NAME__", name);
    let manifest = format!(
        "# {name}: WASM component on WASI 0.2.\n\
         target = \"component\"\n\
         mode = \"command\"\n\
         \n\
         [app]\n\
         source = \"{name}.wat\"\n\
         path = \"{name}.wasm\"\n\
         run = \"wasi:cli/run\"\n"
    );
    (starter, manifest)
}

fn gui_starter(name: &str) -> (String, String) {
    let starter = format!(
        ";; {name}.wat -- native egui app hosted by air.\n\
         ;; The entry runs every UI frame, and is an ordinary component\n\
         ;; export: `mode = \"gui\"` only decides who calls it and how often.\n\
         ;; A named data segment gets $name.ptr and $name.len from air, so\n\
         ;; no string length is ever written by hand.\n\
         (component\n\
         \x20 ;; `ui` is the host's own interface, generated from WIT exactly\n\
         \x20 ;; like a WASI one. Strings cross by value: the canonical ABI\n\
         \x20 ;; does the copy, the bounds check and the UTF-8 validation.\n\
         \x20 ;; @wasi ui pages=1\n\
         \n\
         \x20 (core module $main\n\
         \x20   (import \"env\" \"memory\" (memory 1))\n\
         \x20   (import \"ui\" \"label\" (func $label (param i32 i32)))\n\
         \x20   (import \"ui\" \"button\" (func $button (param i32 i32) (result i32)))\n\
         \x20   ;; @include src/state.wat\n\
         \x20   (global $count (mut i32) (i32.const 0))\n\
         \x20   (func (export \"frame\")\n\
         \x20     (call $label (global.get $title.ptr) (global.get $title.len))\n\
         \x20     (if (call $button (global.get $increment.ptr) (global.get $increment.len))\n\
         \x20       (then (global.set $count (i32.add (global.get $count) (i32.const 1)))))\n\
         \x20     (call $label (global.get $status.ptr) (global.get $status.len)))\n\
         \x20   (data $title (i32.const 0) \"Hello from {name}\")\n\
         \x20   (data $increment (i32.const 256) \"Increment\")\n\
         \x20   (data $status (i32.const 512) \"Button is ready\"))\n\
         \x20 (core instance $app (instantiate $main\n\
         \x20   (with \"env\" (instance $mem))\n\
         \x20   (with \"ui\" (instance $ui))))\n\
         \n\
         \x20 (func $frame (canon lift (core func $app \"frame\")))\n\
         \x20 (export \"frame\" (func $frame))\n\
         )\n"
    );
    let manifest = format!(
        "# {name}: native egui GUI app.\n\
         mode = \"gui\"\n\
         \n\
         [app]\n\
         source = \"{name}.wat\"\n\
         path = \"{name}.wasm\"\n\
         run = \"frame\"\n"
    );
    (starter, manifest)
}

/// Scaffold a manifest stub beside a prebuilt artifact.
///
/// A component needs almost nothing said about it -- its own type section
/// carries the imports and exports, and `air check` reports them -- so this
/// writes the manifest and stops. A Core module is the interesting case: the
/// harness has no Core host any more, so the answer is not a manifest key but
/// a build step, and saying which one is more use than a stub that cannot run.
pub fn cmd_init(engine: &Engine, app_path: &str) -> Result<()> {
    let bytes = std::fs::read(app_path)?;
    if !crate::manifest::is_component_binary(&bytes) {
        // Confirm it is WASM at all before blaming the layer.
        wasmtime::Module::new(engine, &bytes)?;
        return fail(format!(
            "{app_path} is a Core WASM module. The native host has retired, so a \
             manifest cannot host it directly. Lift it into a component:\n\n  \
             wasm-tools component embed <wit-dir> {app_path} -o embedded.wasm\n  \
             wasm-tools component new embedded.wasm -o app.component.wasm\n\n\
             A module built against WASI Preview 1 needs the standard adapter \
             instead: `wasm-tools component new {app_path} --adapt \
             wasi_snapshot_preview1.reactor.wasm`. Then run `air init` on the \
             component, or declare it as a `[[providers]]` entry of an \
             application that imports its interface. `air inspect {app_path}` \
             lists what it exports."
        ));
    }
    let stem = std::path::Path::new(app_path)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "app".into());
    let dir = std::path::Path::new(app_path)
        .parent()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    let pref = if dir.is_empty() {
        String::new()
    } else {
        format!("{dir}/")
    };
    let mut out = String::new();
    out.push_str("mode = \"command\"\n");
    out.push_str("# root = \"www\"       # uncomment to grant a directory\n");
    out.push_str("# network = true    # uncomment to allow sockets\n\n");
    out.push_str("# [[providers]]     # a component this one imports an interface from\n");
    out.push_str("# package = \"namespace:name\"\n# version = \"0.1.0\"\n# Install it once: air add --from <released-package-dir> namespace:name@0.1.0\n\n");
    out.push_str(&format!(
        "[app]\npath = \"{pref}{stem}.wasm\"\nrun = \"wasi:cli/run\"\n"
    ));
    let toml_path = format!("{pref}host.toml");
    // Never silently overwrite an existing manifest.
    if std::path::Path::new(&toml_path).exists() {
        return Err(wasmtime::Error::msg(format!(
            "{toml_path} exists, refusing to overwrite"
        )));
    }
    std::fs::write(&toml_path, &out)?;
    println!("wrote {toml_path}:\n{out}");
    Ok(())
}
