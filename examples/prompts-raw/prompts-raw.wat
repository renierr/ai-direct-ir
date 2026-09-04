;; prompts-raw.wat — raw-terminal Clack-style select using host `term.*`.
;;
;; Imports are the reusable terminal ABI: available, enter/exit alternate
;; screen + raw mode, clear, read_key. stdout remains WASI fd_write because
;; text is still just bytes. Arrow up/down chooses; Enter confirms; Escape or
;; Ctrl-C cancels. The Host Drop guard restores the terminal on a trap too.
;; Memory map: 0x00 iov, 0x08 nwritten, 0x1000 strings.

(module
  (import "wasi_snapshot_preview1" "fd_write"
    (func $write (param i32 i32 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "proc_exit" (func $exit (param i32)))
  (import "term" "available" (func $available (result i32)))
  (import "term" "enter" (func $enter (result i32)))
  (import "term" "exit" (func $term_exit (result i32)))
  (import "term" "clear" (func $clear (result i32)))
  (import "term" "read_key" (func $key (result i32)))
  (memory 1)
  (export "memory" (memory 0))

  (func $print (param $p i32) (param $n i32)
    (i32.store (i32.const 0) (local.get $p))
    (i32.store (i32.const 4) (local.get $n))
    (call $write (i32.const 1) (i32.const 0) (i32.const 1) (i32.const 8))
    (drop))

  (func $draw (param $choice i32)
    (call $clear) (drop)
    (call $print (i32.const 0x1000) (i32.const 58))
    (local.get $choice) (i32.const 0) (i32.eq)
    (if (then (call $print (i32.const 0x103d) (i32.const 19)))
        (else (call $print (i32.const 0x1050) (i32.const 19))))
    (local.get $choice) (i32.const 1) (i32.eq)
    (if (then (call $print (i32.const 0x1063) (i32.const 19)))
        (else (call $print (i32.const 0x1076) (i32.const 19))))
    (local.get $choice) (i32.const 2) (i32.eq)
    (if (then (call $print (i32.const 0x1089) (i32.const 19)))
        (else (call $print (i32.const 0x109c) (i32.const 19)))))

  (func (export "_start")
    (local $choice i32) (local $key i32)
    (call $available) (i32.eqz)
    (if (then
      (call $print (i32.const 0x10af) (i32.const 62))
      (call $exit (i32.const 2))
      (unreachable)))
    (call $enter) (i32.const 0) (i32.ne)
    (if (then (call $exit (i32.const 2)) (unreachable)))
    (local.set $choice (i32.const 0))
    (block $done
      (loop $input
        (call $draw (local.get $choice))
        (local.set $key (call $key))
        (local.get $key) (i32.const 0x102) (i32.eq) ;; down
        (if (then
          (local.set $choice (i32.rem_u
            (i32.add (local.get $choice) (i32.const 1)) (i32.const 3)))
          (br $input)))
        (local.get $key) (i32.const 0x101) (i32.eq) ;; up
        (if (then
          (local.set $choice (i32.rem_u
            (i32.add (local.get $choice) (i32.const 2)) (i32.const 3)))
          (br $input)))
        (local.get $key) (i32.const 0x10d) (i32.eq) ;; enter
        (br_if $done)
        (local.get $key) (i32.const 0x11b) (i32.eq) ;; escape
        (local.get $key) (i32.const 3) (i32.eq)     ;; ctrl-c
        (i32.or)
        (if (then
          (call $term_exit) (drop)
          (call $print (i32.const 0x10ed) (i32.const 11))
          (call $exit (i32.const 1))
          (unreachable)))
        (br $input)))
    (call $term_exit) (drop)
    (call $print (i32.const 0x10f8) (i32.const 10))
    (local.get $choice) (i32.const 0) (i32.eq)
    (if (then (call $print (i32.const 0x1102) (i32.const 3))))
    (local.get $choice) (i32.const 1) (i32.eq)
    (if (then (call $print (i32.const 0x1105) (i32.const 7))))
    (local.get $choice) (i32.const 2) (i32.eq)
    (if (then (call $print (i32.const 0x110c) (i32.const 4))))
    (call $print (i32.const 0x1110) (i32.const 1))
    (call $exit (i32.const 0))
    (unreachable))

  (data (i32.const 0x1000) "prompts raw demo\n\nChoose an environment (arrows, Enter):\n\n")
  (data (i32.const 0x103d) "> dev             \n")
  (data (i32.const 0x1050) "  dev             \n")
  (data (i32.const 0x1063) "> staging         \n")
  (data (i32.const 0x1076) "  staging         \n")
  (data (i32.const 0x1089) "> prod            \n")
  (data (i32.const 0x109c) "  prod            \n")
  (data (i32.const 0x10af) "interactive terminal required; use examples/prompts for pipes\n")
  (data (i32.const 0x10ed) "Cancelled.\n")
  (data (i32.const 0x10f8) "Selected: ")
  (data (i32.const 0x1102) "dev")
  (data (i32.const 0x1105) "staging")
  (data (i32.const 0x110c) "prod")
  (data (i32.const 0x1110) "\n")
)
