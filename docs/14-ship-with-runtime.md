# Shipping to Production with the WASM Runtime

No wasm2c needed. The production artifact is **runtime + `.wasm`**, deployed like any service. Options ordered by complexity:

## Option 1 — Container image (recommended default)

`ship/Dockerfile` pins `wasmtime v48.0.1` + bakes in `pi.wasm`. Build once, run on any host/K8s/edge that runs containers:

```bash
docker build -f ship/Dockerfile -t pi-wasm:1 .
echo 100 | docker run --rm -i pi-wasm:1
```

Why this is nice: image is tiny (runtime ~10 MB + our KB-sized module), immutable, same bytes dev→prod. For fast cold starts, precompile inside the image (`wasmtime compile` → `.cwasm`) — same behavior, quicker boot.

## Option 2 — VM/bare metal via systemd

Copy two files: the `wasmtime` binary + `app.wasm`. `ship/pi-wasi.service` shows the pattern: dedicated `User=`, `NoNewPrivileges`, `ProtectSystem=strict`, and only the capabilities the module needs (`--dir` preopens, no network unless required). Restart/update = replace the `.wasm`, `systemctl restart`. The WASM sandbox is your first layer, systemd hardening the second.

## Option 3 — Kubernetes

WASM runs as containers via the containerd shims (**runwasi**: `wasmtime`/`wamr`/`wasmedge` shims) or specialized operators (**SpinKube** for Spin HTTP apps). Pod spec barely differs from normal — runtime class handles it. Worth it when you already run K8s; overkill otherwise.

## Option 4 — Serverless/edge & PaaS

Fermyon Cloud / Wasmer Edge / Fastly Compute / Cloudflare Workers: push the component, they run it. Fastest to production for HTTP components (`wasi:http` + `wasmtime serve` model from `docs/09`), least control.

## Production checklist (applies to all)

- **Pin everything:** wasmtime version, `.wasm` hash (record in lablog/release notes). Never `latest` in prod.
- **Least capability:** grant only needed preopens/env; run as non-root; set wasmtime limits (fuel, max memory) so a bad module can't spin forever.
- **CI gate:** `wat2wasm` → `wasm-tools validate` → `wasm-opt` → behavioral tests (like our N=1000 cross-check) → build image → push. A module that fails validation never ships.
- **Updates:** `.wasm` files are KBs — rolling deploys are seconds. Keep old version for instant rollback.
- **Observe:** stdout/stderr to your log collector, exit codes to alerts; for HTTP, metrics at the reverse proxy in front of `wasmtime serve`.
