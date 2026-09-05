;; hello.wat -- the smallest AI-direct IR app: a WASI 0.2 component that
;; writes one line to stdout.
;;
;; `;; @wasi stdout` is the whole Component Model boundary. `air` generates
;; the imports, the shared memory and the canonical ABI lowering from it, and
;; hands the application ordinary Core functions on the `wasi` instance.
;;
;; Build: air build examples/hello/host.toml
;; Run:   air run examples/hello/host.toml
;;
;; Memory map (1 page): 0x100 message, 0x200 stream result,
;;                      0x8000+ canonical ABI bump allocation

(component
  ;; @wasi stdout

  ;; --- application logic, ordinary Core WAT -----------------------------
  (core module $main
    (import "env" "memory" (memory 1))
    (import "wasi" "get-stdout" (func $get_stdout (result i32)))
    (import "wasi" "write" (func $write (param i32 i32 i32 i32)))

    ;; A named data segment: `air` derives $msg.ptr and $msg.len from it,
    ;; so the length is never restated and can never go stale.
    (data $msg (i32.const 0x100) "hello from AI-direct IR\n")

    ;; result<_, stream-error> is returned through the 8 bytes at 0x200.
    ;; 0 = ok, 1 = err: forward the stream result as the run result.
    (func (export "run") (result i32)
      (call $write (call $get_stdout)
        (global.get $msg.ptr) (global.get $msg.len) (i32.const 0x200))
      (i32.load (i32.const 0x200))))

  (core instance $app (instantiate $main
    (with "env" (instance $mem))
    (with "wasi" (instance $wasi))))

  ;; `run: func() -> result`: ok is exit 0, err is a failed run.
  (func $run (result (result)) (canon lift (core func $app "run")))
  (instance $run-i (export "run" (func $run)))
  (export "wasi:cli/run@0.2.12" (instance $run-i))
)
