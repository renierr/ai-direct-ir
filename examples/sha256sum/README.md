# sha256sum

A real CLI, to prove the provider path end to end.

```bash
air run examples/sha256sum/host.toml <file>   # digest a file
air run examples/sha256sum/host.toml -        # digest stdin
air run examples/sha256sum/host.toml --help
```

Exit codes: `0` success, `1` usage, `2` I/O failure. Output matches coreutils
`sha256sum` byte for byte, two spaces and all.

## What it proves

- **Someone else's compiled work.** The cryptography is the RustCrypto `sha2`
  crate, built into a component in `ai-direct-ir-providers` and **vendored**
  into `vendor/` with its hash and license notices. No code here computes a
  digest.
- **A WASI interface the harness has never heard of.** The `wasi:filesystem`
  imports in `sha256sum.wat` are hand-written. `air` contains no filesystem
  code; it links the whole WASI 0.2 set, so a new interface is a declaration in
  the application, not a change to the harness.
- **Guest arguments.** `air run <manifest> <args...>` forwards everything after
  the manifest, which is a host policy decision and therefore genuinely the
  harness's job.

## Layout

| Path | |
|---|---|
| `sha256sum.wat` | the whole application: arguments, file reading, output |
| `host.toml` | `root` preopens the directory the app may read |
| `vendor/ai-direct-sha256-0.1.0/` | the vendored provider, hash-locked |

Verify the vendored artifact against its release:

```bash
cd vendor/ai-direct-sha256-0.1.0 && sha256sum -c checksums.txt
```

## Limits

Paths resolve inside the preopened `root`, so the app cannot read outside it —
that is the capability model doing its job, not a bug. Input is buffered whole
at `0x10000..0x40000` (192 KiB); the provider has no streaming digest yet, so a
larger file needs a `hash-stream` resource on the provider's contract.
