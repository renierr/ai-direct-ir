# Production image: pinned wasmtime runtime + our module. No SDK, no source.
FROM debian:bookworm-slim

ARG WASMTIME_VERSION=48.0.1
ADD https://github.com/bytecodealliance/wasmtime/releases/download/v${WASMTIME_VERSION}/wasmtime-v${WASMTIME_VERSION}-x86_64-linux.tar.xz /tmp/wasmtime.tar.xz
RUN tar -xJf /tmp/wasmtime.tar.xz -C /tmp \
 && mv /tmp/wasmtime-v${WASMTIME_VERSION}-x86_64-linux/wasmtime /usr/local/bin/wasmtime \
 && rm -rf /tmp/wasmtime* \
 && wasmtime --version

# Precompiled AOT for fast cold starts (same behavior, quicker boot).
COPY src/pi.wasm /app/pi.wasm
RUN wasmtime compile /app/pi.wasm -o /app/pi.cwasm

USER nobody
ENTRYPOINT ["wasmtime", "run", "--allow-precompiled", "/app/pi.cwasm"]
