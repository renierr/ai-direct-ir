//! `air serve` -- host the project over HTTP.

use wasmtime::Result;

use crate::manifest::Target;

use crate::fail;

use super::manifest_base;

/// Serve a browser app from its manifest directory. Browsers require HTTP to
/// fetch WASM modules, and WebAssembly uses its own content type.
pub fn cmd_serve(path: &str) -> Result<()> {
    use std::io::{Read, Write};
    use std::net::TcpListener;

    let manifest = crate::manifest::load(path)?;
    if manifest.target != Target::Browser {
        return fail(format!(
            "{path} targets native; `air serve` is for browser apps"
        ));
    }
    let base = std::fs::canonicalize(manifest_base(path))?;
    let port = manifest.port.unwrap_or(8000);
    let listener = TcpListener::bind(("127.0.0.1", port))
        .map_err(|e| wasmtime::Error::msg(format!("bind 127.0.0.1:{port}: {e}")))?;
    println!(
        "serving {} at http://127.0.0.1:{port}/ (Ctrl-C to stop)",
        base.display()
    );
    for stream in listener.incoming() {
        let mut stream = match stream {
            Ok(stream) => stream,
            Err(e) => {
                eprintln!("accept: {e}");
                continue;
            }
        };
        let mut request = [0; 4096];
        let n = match stream.read(&mut request) {
            Ok(n) => n,
            Err(_) => continue,
        };
        let request = String::from_utf8_lossy(&request[..n]);
        let target = request
            .lines()
            .next()
            .and_then(|line| line.split_whitespace().nth(1))
            .unwrap_or("/");
        let rel = target
            .split('?')
            .next()
            .unwrap_or("/")
            .trim_start_matches('/');
        let candidate = if rel.is_empty() {
            base.join("index.html")
        } else {
            base.join(rel)
        };
        let file = std::fs::canonicalize(&candidate)
            .ok()
            .filter(|file| file.starts_with(&base));
        match file.and_then(|file| std::fs::read(&file).ok().map(|body| (file, body))) {
            Some((file, body)) => {
                let content_type = match file.extension().and_then(|s| s.to_str()) {
                    Some("wasm") => "application/wasm",
                    Some("js") => "text/javascript; charset=utf-8",
                    Some("html") => "text/html; charset=utf-8",
                    Some("css") => "text/css; charset=utf-8",
                    _ => "application/octet-stream",
                };
                let _ = write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nCache-Control: no-cache\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(&body);
            }
            None => {
                let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n");
            }
        }
    }
    Ok(())
}
