;; demo-tool.wat — demo-tool app, hosted by host-rs.
;; Build: wat2wasm demo-tool.wat -o demo-tool.wasm
;; Check: host-rs check demo-tool.toml
;; Run:   host-rs demo-tool.toml
;;
;; Command-mode contract: own memory (export it for WASI),
;; WASI stdio, `_start` entry, `proc_exit` code is the exit code.
;; Need sockets, shared libs, or bridges? New needs go in the
;; manifest (TOML), never in harness code.

(module
  (import "wasi_snapshot_preview1" "fd_write"
    (func $fd_write (param i32 i32 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "proc_exit"
    (func $exit (param i32)))
  (memory 1)
  (export "memory" (memory 0))

  (func (export "_start")
    (i32.store (i32.const 0) (i32.const 0x1000))
    (i32.store (i32.const 4) (i32.const 21))
    (call $fd_write (i32.const 1) (i32.const 0)
      (i32.const 1) (i32.const 8))
    (drop)
    (call $exit (i32.const 0))
    (unreachable))

  (data (i32.const 0x1000) "hello from demo-tool\n")
)
