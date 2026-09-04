# Source Layout

`<app>.wat` in the project root is the one source declared by `host.toml`; it opens the module,
declares imports/memory/exports, and orders the application's fragments. Keep it
small enough to be an index, not a second home for every feature.

Place application-owned WAT by responsibility under `src/` as the application
grows:

```text
<app>.wat               module boundary and ordered includes
src/
  state.wat             globals, records-in-memory, state transitions
  input.wat             validation and decoding of untrusted input
  domain/               product rules, one concern per file
  views/                rendering/layout per screen or feature
  providers/            thin call adapters for declared providers
  strings.wat           stable text/data blocks when they need ownership
```

`host-rs` expands a standalone line of the form
`;; @include relative/path.wat` in the root WAT source before assembling. Includes are
ordered, relative to the including root file, project-local, and textual: their
combined content must form one valid `(module ...)`. A fragment should not open
or close the module. Edit a fragment, then use `host-rs check` or `host-rs run`;
both rebuild when any included fragment is newer than the generated WASM.

Do not split one application into Core WASM modules merely for source
organization. A separate module is a provider with an explicit manifest ABI.
