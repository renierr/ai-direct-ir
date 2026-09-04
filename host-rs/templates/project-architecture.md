# __APPNAME__ Architecture

## Application Boundary

Keep application policy, state transitions, and presentation behavior in WAT.
Use providers for mature implementations such as storage, protocols, codecs,
and platform integration. Do not change `host-rs` merely because this app needs
a library.

## State

- List persistent state, transient state, ownership, and recovery behavior.

## Providers And Capabilities

| Need | Provider or built-in capability | Status | Decision |
|---|---|---|---|
| Example | `ai-direct:example` | Planned | Replace with the actual need. |

Declare only locally vendored provider artifacts in `host.toml`. Record their
WIT contract, source/provenance, license, hash, and required permissions.

## Trust Boundary

- Identify untrusted inputs and how the application handles them.
- Identify sensitive data and ensure it is ignored rather than committed.
- State filesystem, network, credentials, or device requirements explicitly.
