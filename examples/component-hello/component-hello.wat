;; component-hello.wat -- a WASI 0.2 command component, written by hand.
;; Proves an AI can author the Component Model boundary with no bindings
;; generator and no language toolchain: host-rs assembles this directly.

(component
  ;; --- imported WASI 0.2 interfaces -------------------------------------
  (import "wasi:io/error@0.2.12" (instance $io-error
    (export "error" (type (sub resource)))))
  (alias export $io-error "error" (type $error))

  (import "wasi:io/streams@0.2.12" (instance $streams
    (export "error" (type $ie (eq $error)))
    (export "output-stream" (type $os (sub resource)))
    (type $se (variant (case "last-operation-failed" (own $ie)) (case "closed")))
    (export "stream-error" (type $sexp (eq $se)))
    (export "[method]output-stream.blocking-write-and-flush"
      (func (param "self" (borrow $os)) (param "contents" (list u8))
            (result (result (error $sexp)))))))
  (alias export $streams "output-stream" (type $ostream))
  (alias export $streams "[method]output-stream.blocking-write-and-flush" (func $write))

  (import "wasi:cli/stdout@0.2.12" (instance $stdout
    (export "output-stream" (type (eq $ostream)))
    (export "get-stdout" (func (result (own $ostream))))))
  (alias export $stdout "get-stdout" (func $get-stdout))

  ;; --- memory lives in its own core module ------------------------------
  ;; Lowering an import needs the memory, and the main module needs the
  ;; lowered imports. Sharing one memory module breaks that cycle.
  (core module $mem-mod (memory (export "memory") 1))
  (core instance $mem (instantiate $mem-mod))
  (alias core export $mem "memory" (core memory $memory))

  (core func $get-stdout-lowered (canon lower (func $get-stdout)))
  (core func $write-lowered (canon lower (func $write) (memory $memory)))
  (core instance $wasi (export "get-stdout" (func $get-stdout-lowered))
                       (export "write" (func $write-lowered)))

  ;; --- application logic, ordinary Core WAT -----------------------------
  (core module $main
    (import "env" "memory" (memory 1))
    (import "wasi" "get-stdout" (func $get-stdout (result i32)))
    (import "wasi" "write" (func $write (param i32 i32 i32 i32)))

    ;; result<_, stream-error> is returned through a 8-byte area at 0x200.
    (func (export "run") (result i32)
      (call $write
        (call $get-stdout)
        (global.get $msg.ptr) (global.get $msg.len)
        (i32.const 0x200))
      ;; 0 = ok, 1 = err: forward the stream result as the run result.
      (i32.load (i32.const 0x200)))

    (data $msg (i32.const 0x100) "hello from AI-direct IR\n")
  )
  (core instance $app (instantiate $main
    (with "env" (instance $mem))
    (with "wasi" (instance $wasi))))

  ;; --- exported wasi:cli/run -------------------------------------------
  (func $run (result (result)) (canon lift (core func $app "run")))
  (instance $run-instance (export "run" (func $run)))
  (export "wasi:cli/run@0.2.12" (instance $run-instance))
)
