;; A consumer component: imports ai-direct:demo/text and prints the result.
;; host-rs satisfies that import by forwarding into the provider component.

(component
  ;; @wasi stdout

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
