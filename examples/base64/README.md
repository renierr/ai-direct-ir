# base64

A small application that demonstrates a pure WAT provider package. The app
imports `ai-direct:base64/codec@0.1.0`, asks it to encode `foobar`, and prints
the RFC 4648 output `Zm9vYmFy`.

Install the locked provider once after cloning, then run it:

```bash
cd examples/base64
air add --from ../../../ai-direct-ir-providers/providers/base64 ai-direct:base64@0.1.0
air run
```

The application contains only WAT, `host.toml`, and `air.lock`; the reviewed
provider package is cached under `~/.cache/air/providers/<artifact-hash>/`.
