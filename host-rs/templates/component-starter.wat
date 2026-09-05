;; hello-comp.wat -- a WASI 0.2 command component.
;;
;; `;; @wasi stdout` is the whole Component Model boundary: host-rs generates
;; the interface imports, the shared memory and the canonical ABI lowering, so
;; the application below is ordinary Core WAT. Add capabilities to that line as
;; the program needs them (`stdin`, `stderr`, `exit`), and size the memory with
;; `pages=` / `heap=`.
;;
;; Memory map (1 page): 0x100 message, 0x200 stream result,
;;                      0x8000+ canonical ABI bump allocation

(component
  ;; @wasi stdout

  ;; --- application logic, ordinary Core WAT -----------------------------
  (core module $main
    (import "env" "memory" (memory 1))
    (import "wasi" "get-stdout" (func $get-stdout (result i32)))
    (import "wasi" "write" (func $write (param i32 i32 i32 i32)))

    ;; Name the segment and read $msg.ptr / $msg.len; never count bytes.
    (data $msg (i32.const 0x100) "hello from __NAME__\n")

    ;; result<_, stream-error> is returned through the 8 bytes at 0x200.
    ;; 0 = ok, 1 = err: forward the stream result as the run result.
    (func (export "run") (result i32)
      (call $write
        (call $get-stdout)
        (global.get $msg.ptr) (global.get $msg.len)
        (i32.const 0x200))
      (i32.load (i32.const 0x200)))
  )
  (core instance $app (instantiate $main
    (with "env" (instance $mem))
    (with "wasi" (instance $wasi))))

  ;; --- exported wasi:cli/run -------------------------------------------
  (func $run (result (result)) (canon lift (core func $app "run")))
  (instance $run-instance (export "run" (func $run)))
  (export "wasi:cli/run@0.2.12" (instance $run-instance))
)
