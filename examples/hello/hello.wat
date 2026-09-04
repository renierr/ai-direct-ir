;; hello.wat -- the smallest AI-direct IR app: a WASI 0.2 component that
;; writes one line to stdout.
;;
;; Everything below is hand-written WAT. The Component Model boundary needs
;; no bindings generator and no language toolchain; host-rs assembles this
;; source in-process.
;;
;; Build: host-rs build examples/hello/host.toml
;; Run:   host-rs run examples/hello/host.toml
;;
;; Memory map (1 page): 0x100 message, 0x200 stream result,
;;                      0x8000+ canonical ABI bump allocation

(component
  ;; --- WASI 0.2 boundary ------------------------------------------------
  ;; Written by hand: declaring the interfaces, lowering them into Core
  ;; functions, and lifting the entry point back out is all the Component
  ;; Model boundary is. No bindings generator is involved.
  ;;
  ;; A function signature must reference the *exported* type id (`$sexp`),
  ;; not the local type it was defined from, or validation rejects the
  ;; whole instance.
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

  ;; --- shared memory ----------------------------------------------------
  ;; Lowering an import needs the memory; the logic module needs the lowered
  ;; imports. A separate memory module breaks that cycle.
  (core module $mem-mod
    (memory (export "memory") 1)
    (global $bump (mut i32) (i32.const 0x8000))
    ;; The canonical ABI allocates host-produced values here. A bump
    ;; allocator is enough: this program never frees.
    (func (export "cabi_realloc")
      (param $old i32) (param $old_size i32) (param $align i32) (param $new i32)
      (result i32)
      (local $ptr i32)
      (global.set $bump
        (i32.and (i32.add (global.get $bump) (i32.sub (local.get $align) (i32.const 1)))
                 (i32.xor (i32.sub (local.get $align) (i32.const 1)) (i32.const -1))))
      (local.set $ptr (global.get $bump))
      (global.set $bump (i32.add (global.get $bump) (local.get $new)))
      (local.get $ptr)))
  (core instance $mem (instantiate $mem-mod))
  (alias core export $mem "memory" (core memory $memory))
  (alias core export $mem "cabi_realloc" (core func $realloc))

  (core func $get-stdout-l (canon lower (func $get-stdout)))
  (core func $write-l (canon lower (func $write) (memory $memory) (realloc $realloc)))
  (core instance $wasi
    (export "get-stdout" (func $get-stdout-l))
    (export "write" (func $write-l)))

  ;; --- application logic, ordinary Core WAT -----------------------------
  (core module $main
    (import "env" "memory" (memory 1))
    (import "wasi" "get-stdout" (func $get_stdout (result i32)))
    (import "wasi" "write" (func $write (param i32 i32 i32 i32)))

    ;; A named data segment: host-rs derives $msg.ptr and $msg.len from it,
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
