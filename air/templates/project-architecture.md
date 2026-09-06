# __APPNAME__ Architecture

## Application Boundary

Keep application policy, state transitions, and presentation behavior in WAT.
Use providers for mature implementations such as storage, protocols, codecs,
and platform integration. Do not change `air` merely because this app needs
a library.

## Source Structure

Keep the root WAT source as the module boundary and ordered include list. Put
feature-sized WAT fragments under `src/` by responsibility: state, input,
domain rules, views, strings, and provider call adapters. Use one Core module
for application source organization; create another module only for a declared
provider with an explicit ABI.

## State

- List persistent state, transient state, ownership, and recovery behavior.

## Providers And Capabilities

| Need | Provider or built-in capability | Status | Decision |
|---|---|---|---|
| Example | `ai-direct:example` | Planned | Replace with the actual need. |

Declare released provider package/version pairs in `host.toml`, then commit
`air.lock`; it records the WIT contract, registry provenance, license and
hashes. Use local `source`/`path` entries only while developing a provider.

## Trust Boundary

- Identify untrusted inputs and how the application handles them.
- Identify sensitive data and ensure it is ignored rather than committed.
- State filesystem, network, credentials, or device requirements explicitly.
