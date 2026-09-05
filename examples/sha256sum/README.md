# sha256sum

A real CLI, to prove the provider path end to end.

```bash
air run --dir . examples/sha256sum/host.toml <file>   # digest a file
air run examples/sha256sum/host.toml -                # digest stdin
air run examples/sha256sum/host.toml --help
```

**A file has to be granted before it can be read.** WASI has no global
filesystem root: a component reaches only the directories the host preopened
for it, and an absolute path is not a path to anywhere on its own. Grant one
of:

| | |
|---|---|
| `root = "../.."` in `host.toml` | this example's grant: the repository, so a repo-relative path just works |
| `air run --dir . <manifest> <file>` | the directory you ran from |
| `air run --dir / <manifest> /abs/path` | everything; absolute paths work as written |
| `air run --dir-rw <path> ...` | granted for writing too |

Two anchors, on purpose. **Manifest paths are project-relative** -- resolved
against `host.toml`, not against your shell -- so a grant names the same
directory wherever you run from and travels with the app through `air dist`.
**Command-line grants are relative to your shell**, because that is where you
are looking.

Without a grant the app names the file it could not open and prints these
options, rather than failing with nothing to act on.

Exit codes: `0` success, `1` usage, `2` I/O failure. Output matches coreutils
`sha256sum` byte for byte, two spaces and all.

## What it proves

- **Someone else's compiled work.** The cryptography is the RustCrypto `sha2`
  crate, built into a component in `ai-direct-ir-providers` and **vendored**
  into `vendor/` with its hash and license notices. No code here computes a
  digest.
- **A WASI interface the harness has never heard of.** `air` contains no
  filesystem code: `;; @wasi ... filesystem` generates the `wasi:filesystem`
  boundary from the vendored WASI WIT, and the three `(import "fs" ...)` lines
  in `sha256sum.wat` decide its extent. `air` links the whole WASI 0.2 set, so
  an interface the directive does not know is still a declaration in the
  application, not a change to the harness.
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

Paths resolve inside a granted directory, so the app cannot read outside one —
that is the capability model doing its job, not a bug. An absolute argument has
its leading `/` stripped and is then tried against each grant, which is what
makes `--dir /` behave the way a shell user expects. Input is buffered whole
at `0x10000..0x40000` (192 KiB); the provider has no streaming digest yet, so a
larger file needs a `hash-stream` resource on the provider's contract.
