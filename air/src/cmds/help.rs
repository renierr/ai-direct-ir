//! `air help` -- the command list the CLI prints with no arguments.

use crossterm::{
    execute,
    style::{Attribute, Color, Print, ResetColor, SetAttribute, SetForegroundColor},
};
use std::io::{IsTerminal, Write};

pub fn print_help() {
    let mut out = std::io::stdout();
    let color = out.is_terminal();
    if color {
        let _ = execute!(
            out,
            SetForegroundColor(Color::Cyan),
            SetAttribute(Attribute::Bold),
            Print("air"),
            SetAttribute(Attribute::Reset),
            ResetColor,
            Print(format!(
                " {} -- build, validate, and run component, GUI, or browser WASM projects\n",
                env!("CARGO_PKG_VERSION")
            )),
        );
    } else {
        let _ = writeln!(
            out,
            "air {} -- build, validate, and run component, GUI, or browser WASM projects",
            env!("CARGO_PKG_VERSION")
        );
    }
    help_section(&mut out, color, "USAGE");
    let _ = writeln!(
        out,
        "  air [command] [args]      use host.toml in a project directory or pass a path"
    );
    help_section(&mut out, color, "COMMANDS");
    for (command, description) in [
        (
            "build [manifest.toml]",
            "assemble app.source into app.path; defaults to host.toml",
        ),
        (
            "dist [manifest.toml]",
            "create a self-contained dist/ bundle; defaults to host.toml",
        ),
        (
            "run [manifest.toml]",
            "link and execute a component; defaults to host.toml",
        ),
        (
            "serve [manifest.toml]",
            "serve a browser app on localhost; defaults to host.toml",
        ),
        ("<manifest.toml>", "shorthand for `run`"),
        (
            "check [manifest.toml]",
            "link everything, verify wiring; defaults to host.toml",
        ),
        (
            "inspect <module.wasm>",
            "show imports/exports for a prebuilt module",
        ),
        (
            "init <app.component.wasm>",
            "write a non-overwriting host.toml stub beside a component",
        ),
        (
            "new <name>",
            "choose component, browser, or GUI; scaffold a project",
        ),
        ("help, -h, --help", "this text"),
        ("version, -V, --version", "version"),
    ] {
        help_line(&mut out, color, command, description);
    }
    help_section(&mut out, color, "EXAMPLES");
    for (command, description) in [
        ("air new myapp", "choose component, browser, or GUI"),
        ("cd myapp && air build", ""),
        ("air check", "validate host.toml and the compiled app"),
        ("air dist", "create a shippable dist/ bundle"),
        ("air run", "component or GUI project"),
        ("air serve", "browser project"),
        (
            "air inspect external-lib.wasm",
            "inspect a prebuilt module's ABI",
        ),
        (
            "air init app.component.wasm",
            "write a host.toml stub beside it",
        ),
    ] {
        help_line(&mut out, color, command, description);
    }
    // Argument order is the one thing about the CLI that is not guessable:
    // the manifest is the divider, and everything past it belongs to the app.
    help_section(&mut out, color, "ARGUMENTS");
    for (command, description) in [
        (
            "air run --dir . host.toml a b",
            "host options come BEFORE the manifest",
        ),
        (
            "",
            "everything after it goes to the app, argv[0] = the app name",
        ),
        (
            "--dir <p> / --dir-rw <p>",
            "grant a directory, relative to the shell",
        ),
        (
            "--net",
            "grant sockets; a manifest asks with `network = true`",
        ),
    ] {
        help_line(&mut out, color, command, description);
    }
}

fn help_section(out: &mut std::io::Stdout, color: bool, title: &str) {
    let _ = writeln!(out);
    if color {
        let _ = execute!(
            out,
            SetForegroundColor(Color::Yellow),
            SetAttribute(Attribute::Bold),
            Print(title),
            SetAttribute(Attribute::Reset),
            ResetColor,
            Print(":\n"),
        );
    } else {
        let _ = writeln!(out, "{title}:");
    }
}

fn help_line(out: &mut std::io::Stdout, color: bool, command: &str, description: &str) {
    if color {
        let _ = execute!(
            out,
            Print("  "),
            SetForegroundColor(Color::Green),
            Print(format!("{command:<34}")),
            ResetColor,
            SetForegroundColor(Color::DarkGrey),
            Print(description),
            ResetColor,
            Print("\n"),
        );
    } else {
        let _ = writeln!(out, "  {command:<34}{description}");
    }
}
