;; A consumer component: imports ai-direct:demo/text and prints the result.
;; host-rs satisfies that import by forwarding into the provider component.

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

  ;; --- the provider interface, imported like any other -------------------
  (import "ai-direct:demo/text" (instance $text
    (export "shout" (func (param "text" string) (result string)))))
  (alias export $text "shout" (func $shout))
  (core func $shout-l (canon lower (func $shout) (memory $memory) (realloc $realloc)))
  (core instance $prov (export "shout" (func $shout-l)))

  ;; --- application logic, ordinary Core WAT -----------------------------
  (core module $main
    (import "env" "memory" (memory 1))
    (import "wasi" "get-stdout" (func $get_stdout (result i32)))
    (import "wasi" "write" (func $write (param i32 i32 i32 i32)))
    (import "provider" "shout" (func $shout (param i32 i32 i32)))

    (data $greeting (i32.const 0x100) "hello from a provider\n")

    (func (export "run") (result i32)
      ;; shout(ptr, len, retptr); the return area at 0x300 holds [ptr, len].
      (call $shout (global.get $greeting.ptr) (global.get $greeting.len)
        (i32.const 0x300))
      (call $write (call $get_stdout)
        (i32.load (i32.const 0x300)) (i32.load (i32.const 0x304))
        (i32.const 0x200))
      (i32.load (i32.const 0x200))))
  (core instance $app (instantiate $main
    (with "env" (instance $mem))
    (with "wasi" (instance $wasi))
    (with "provider" (instance $prov))))

  (func $run (result (result)) (canon lift (core func $app "run")))
  (instance $run-i (export "run" (func $run)))
  (export "wasi:cli/run@0.2.12" (instance $run-i))
)
