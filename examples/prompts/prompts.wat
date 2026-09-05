;; examples/prompts/prompts.wat -- clack-style CLI prompts, line-based.
;;
;; A WASI 0.2 component: stdio comes from wasi:cli, exit from wasi:cli/exit,
;; and the entry point is wasi:cli/run. No harness services are used.
;; Flow (dummy project setup): intro, text (name), select (env),
;; multiselect (features), confirm, outro summary. Cancel -> exit 1,
;; I/O error -> exit 2. Fully scriptable: pipe answers on stdin.
;;
;; Finished TUI behavior belongs in a declared project provider; raw-mode
;; termios has no WASI 0.2 interface, so instant keypress input is not
;; available here. ANSI output would work: it is just bytes.
;;
;; Memory map: 0x08 write result, 0x0C bytes read, 0x10 read result,
;; 0x100 input line (256B), 0x200 parked name,
;; 0x1000..0x8000 `;; @data` region: `air` places the strings and derives
;; every .ptr/.len, so no address or length is written by hand,
;; 0x8000+ canonical ABI bump allocation

(component
  ;; @wasi stdin stdout exit-with-code

  ;; --- application logic, ordinary Core WAT -----------------------------
  (core module $main
    (import "env" "memory" (memory 1))
    (import "wasi" "get-stdin" (func $get_stdin (result i32)))
    (import "wasi" "get-stdout" (func $get_stdout (result i32)))
    (import "wasi" "read" (func $read (param i32 i64 i32)))
    (import "wasi" "write" (func $write (param i32 i32 i32 i32)))
    ;; wasi:cli/exit replaces proc_exit: $abort and the cancel path unwind
    ;; from deep inside the prompt flow, where a return cannot reach.
    ;; `exit-with-code`, not `exit`: `exit` takes a `result`, so only 0 and 1
    ;; are representable and any other value traps on the discriminant.
    (import "wasi" "exit-with-code" (func $exit (param i32)))

  (func $print (param $p i32) (param $n i32)
    (call $write (call $get_stdout)
      (local.get $p) (local.get $n) (i32.const 0x08)))

  ;; read_one(ptr) -> 0 ok, non-zero error. Bytes read land at 0x0C, so the
  ;; caller reads it exactly as it read the Preview 1 nread.
  (func $read_one (param $ptr i32) (result i32)
    (call $read (call $get_stdin) (i64.const 1) (i32.const 0x10))
    ;; The discriminant is a u8: `i32.load` would read three bytes of
    ;; undefined padding along with the tag.
    (if (i32.load8_u (i32.const 0x10)) (then (return (i32.const 1))))
    (i32.store (i32.const 0x0C) (i32.load (i32.const 0x18)))
    (if (i32.load (i32.const 0x0C))
      (then (i32.store8 (local.get $ptr)
              (i32.load8_u (i32.load (i32.const 0x14))))))
    (i32.const 0))

  (func $abort (param $p i32) (param $n i32) (param $code i32)
    (call $print (local.get $p) (local.get $n))
    (call $exit (local.get $code))
    (unreachable))

  ;; 1 if [a,a+n) == [b,b+n) else 0
  (func $eq (param $a i32) (param $b i32) (param $n i32) (result i32)
    (local $i i32)
    (local.set $i (i32.const 0))
    (loop $l
      (local.get $i) (local.get $n) (i32.ge_u)
      (if (then (i32.const 1) (return)))
      (i32.load8_u (i32.add (local.get $a) (local.get $i)))
      (i32.load8_u (i32.add (local.get $b) (local.get $i)))
      (i32.ne)
      (if (then (i32.const 0) (return)))
      (local.set $i (i32.add (local.get $i) (i32.const 1)))
      (br $l))
    (i32.const 1))

  ;; read a line (without \n, \r stripped) into 0x100: len,
  ;; -1 on I/O error or EOF-before-any-byte (empty line returns 0).
  ;; Byte-at-a-time: a chunked read would swallow bytes meant for later
  ;; prompts (already consumed, sitting in OUR buffer, while the next
  ;; prompt sees EOF). Prompts are human-speed; syscalls are cheap here.
  (func $read_line (result i32)
    (local $total i32) (local $r i32) (local $c i32)
    (local.set $total (i32.const 0))
    (block $done
      (loop $rd
        (local.get $total) (i32.const 256) (i32.ge_u) (br_if $done)
        (local.set $r (call $read_one
          (i32.add (i32.const 0x100) (local.get $total))))
        (local.get $r) (i32.const 0) (i32.ne)
        (if (then (i32.const -1) (return)))
        (local.get $total) (i32.eqz)
        (i32.load (i32.const 0x0C)) (i32.eqz)
        (i32.and)
        (if (then (i32.const -1) (return)))
        (i32.load (i32.const 0x0C)) (i32.eqz)
        (if (then (br $done)))
        (local.set $c
          (i32.load8_u (i32.add (i32.const 0x100) (local.get $total))))
        (local.get $c) (i32.const 10) (i32.eq)
        (if (then
          (local.get $total) (i32.const 0) (i32.gt_u)
          (if (then
            (i32.load8_u (i32.sub
              (i32.add (i32.const 0x100) (local.get $total))
              (i32.const 1)))
            (i32.const 13) (i32.eq)
            (if (then
              (local.set $total
                (i32.sub (local.get $total) (i32.const 1)))))))
          (local.get $total) (return)))
        (local.set $total (i32.add (local.get $total) (i32.const 1)))
        (br $rd)))
    (local.get $total))

  ;; unsigned decimal in [p,p+n), spaces tolerated: value, or -1.
  (func $parse_uint (param $p i32) (param $n i32) (result i32)
    (local $i i32) (local $v i32) (local $c i32) (local $d i32)
    (local.set $i (i32.const 0))
    (block $sp
      (loop $s
        (br_if $sp (i32.ge_u (local.get $i) (local.get $n)))
        (i32.load8_u (i32.add (local.get $p) (local.get $i)))
        (i32.const 32) (i32.ne) (br_if $sp)
        (local.set $i (i32.add (local.get $i) (i32.const 1)))
        (br $s)))
    (local.set $v (i32.const 0))
    (local.set $d (i32.const 0))
    (block $done
      (loop $g
        (br_if $done (i32.ge_u
          (i32.add (local.get $i) (local.get $d)) (local.get $n)))
        (local.set $c (i32.load8_u (i32.add (i32.add
          (local.get $p) (local.get $i)) (local.get $d))))
        (br_if $done (i32.lt_u (local.get $c) (i32.const 48)))
        (br_if $done (i32.gt_u (local.get $c) (i32.const 57)))
        (local.set $v (i32.add (i32.mul (local.get $v) (i32.const 10))
          (i32.sub (local.get $c) (i32.const 48))))
        (local.get $v) (i32.const 1000000) (i32.gt_u)
        (if (then (i32.const -1) (return)))
        (local.set $d (i32.add (local.get $d) (i32.const 1)))
        (br $g)))
    (local.get $d) (i32.eqz)
    (if (then (i32.const -1) (return)))
    (local.set $i (i32.add (local.get $i) (local.get $d)))
    (block $tl
      (loop $t
        (br_if $tl (i32.ge_u (local.get $i) (local.get $n)))
        (i32.load8_u (i32.add (local.get $p) (local.get $i)))
        (i32.const 32) (i32.ne)
        (if (then (i32.const -1) (return)))
        (local.set $i (i32.add (local.get $i) (i32.const 1)))
        (br $t)))
    (local.get $v))

  ;; "1,3" / "2 3" over [p,p+n) -> bitmask, or -1 on any bad token.
  ;; Empty selection yields mask 0 (caller decides if that is ok).
  (func $parse_mask (param $p i32) (param $n i32) (result i32)
    (local $i i32) (local $cur i32) (local $have i32)
    (local $mask i32) (local $c i32)
    (local.set $i (i32.const 0))
    (local.set $cur (i32.const 0))
    (local.set $have (i32.const 0))
    (local.set $mask (i32.const 0))
    (block $end
      (loop $l
        (br_if $end (i32.ge_u (local.get $i) (local.get $n)))
        (local.set $c
          (i32.load8_u (i32.add (local.get $p) (local.get $i))))
        (local.get $c) (i32.const 13) (i32.eq)
        (if (then
          (local.set $i (i32.add (local.get $i) (i32.const 1)))
          (br $l)))
        (i32.and
          (i32.ge_u (local.get $c) (i32.const 48))
          (i32.le_u (local.get $c) (i32.const 57)))
        (if (then
          (local.set $cur (i32.add (i32.mul (local.get $cur)
            (i32.const 10))
            (i32.sub (local.get $c) (i32.const 48))))
          (local.get $cur) (i32.const 100) (i32.gt_u)
          (if (then (i32.const -1) (return)))
          (local.set $have (i32.const 1))
          (local.set $i (i32.add (local.get $i) (i32.const 1)))
          (br $l)))
        (i32.or
          (i32.eq (local.get $c) (i32.const 44))
          (i32.or
            (i32.eq (local.get $c) (i32.const 32))
            (i32.eq (local.get $c) (i32.const 9))))
        (if (then
          (local.get $have)
          (if (then
            (local.get $cur) (i32.const 1) (i32.lt_u)
            (if (then (i32.const -1) (return)))
            (local.get $cur) (i32.const 3) (i32.gt_u)
            (if (then (i32.const -1) (return)))
            (local.set $mask (i32.or (local.get $mask)
              (i32.shl (i32.const 1)
                (i32.sub (local.get $cur) (i32.const 1)))))
            (local.set $cur (i32.const 0))
            (local.set $have (i32.const 0))))
          (local.set $i (i32.add (local.get $i) (i32.const 1)))
          (br $l)))
        (i32.const -1) (return)))
    (local.get $have)
    (if (then
      (local.get $cur) (i32.const 1) (i32.lt_u)
      (if (then (i32.const -1) (return)))
      (local.get $cur) (i32.const 3) (i32.gt_u)
      (if (then (i32.const -1) (return)))
      (local.set $mask (i32.or (local.get $mask)
        (i32.shl (i32.const 1)
          (i32.sub (local.get $cur) (i32.const 1)))))))
    (local.get $mask))

  (func (export "run") (result i32)
    (local $n i32) (local $v i32) (local $mask i32)
    (local $np i32) (local $nl i32) (local $ep i32) (local $el i32)
    (call $print (global.get $intro.ptr) (global.get $intro.len))   ;; intro
    ;; --- text: project name, empty = default ---
    (call $print (global.get $ask-name.ptr) (global.get $ask-name.len))
    (local.set $n (call $read_line))
    (local.get $n) (i32.const 0) (i32.lt_s)
    (if (then (call $abort (global.get $input-closed.ptr) (global.get $input-closed.len) (i32.const 2))))
    (local.get $n) (i32.eqz)
    (if
      (then
        (local.set $np (global.get $default-name.ptr))
        (local.set $nl (global.get $default-name.len)))
      (else
        ;; park the name at 0x200: the 0x100 line buffer is reused
        ;; by every later prompt and would clobber it before $exit.
        (memory.copy (i32.const 0x200) (i32.const 0x100) (local.get $n))
        (local.set $np (i32.const 0x200))
        (local.set $nl (local.get $n))))
    (call $print (global.get $label-name.ptr) (global.get $label-name.len))
    (call $print (local.get $np) (local.get $nl))
    (call $print (global.get $newline.ptr) (global.get $newline.len))
    ;; --- select: environment 1-3 ---
    (call $print (global.get $ask-env.ptr) (global.get $ask-env.len))
    (call $print (global.get $env-1.ptr) (global.get $env-1.len))
    (call $print (global.get $env-2.ptr) (global.get $env-2.len))
    (call $print (global.get $env-3.ptr) (global.get $env-3.len))
    (block $envok
      (loop $env
        (call $print (global.get $prompt.ptr) (global.get $prompt.len))
        (local.set $n (call $read_line))
        (local.get $n) (i32.const 0) (i32.lt_s)
        (if (then
          (call $abort (global.get $input-closed.ptr) (global.get $input-closed.len) (i32.const 2))))
        (local.set $v
          (call $parse_uint (i32.const 0x100) (local.get $n)))
        (i32.and
          (i32.ge_u (local.get $v) (i32.const 1))
          (i32.le_u (local.get $v) (i32.const 3)))
        (if
          (then
            (local.set $ep (global.get $dev.ptr))
            (local.set $el (global.get $dev.len))
            (local.get $v) (i32.const 2) (i32.eq)
            (if (then
              (local.set $ep (global.get $staging.ptr))
              (local.set $el (global.get $staging.len))))
            (local.get $v) (i32.const 3) (i32.eq)
            (if (then
              (local.set $ep (global.get $prod.ptr))
              (local.set $el (global.get $prod.len))))
            (br $envok)))
        (call $print (global.get $env-range.ptr) (global.get $env-range.len))
        (br $env)))
    (call $print (global.get $label-env.ptr) (global.get $label-env.len))
    (call $print (local.get $ep) (local.get $el))
    (call $print (global.get $newline.ptr) (global.get $newline.len))
    ;; --- multiselect: features, at least one ---
    (call $print (global.get $ask-features.ptr) (global.get $ask-features.len))
    (call $print (global.get $feat-1.ptr) (global.get $feat-1.len))
    (call $print (global.get $feat-2.ptr) (global.get $feat-2.len))
    (call $print (global.get $feat-3.ptr) (global.get $feat-3.len))
    (block $fok
      (loop $f
        (call $print (global.get $prompt.ptr) (global.get $prompt.len))
        (local.set $n (call $read_line))
        (local.get $n) (i32.const 0) (i32.lt_s)
        (if (then
          (call $abort (global.get $input-closed.ptr) (global.get $input-closed.len) (i32.const 2))))
        (local.set $mask
          (call $parse_mask (i32.const 0x100) (local.get $n)))
        (i32.and
          (i32.ge_s (local.get $mask) (i32.const 0))
          (i32.ne (local.get $mask) (i32.const 0)))
        (if (then (br $fok)))
        (call $print (global.get $feat-hint.ptr) (global.get $feat-hint.len))
        (br $f)))
    (local.get $mask) (i32.const 1) (i32.and)
    (if (then
      (call $print (global.get $bullet.ptr) (global.get $bullet.len))
      (call $print (global.get $workers.ptr) (global.get $workers.len))
      (call $print (global.get $newline.ptr) (global.get $newline.len))))
    (local.get $mask) (i32.const 2) (i32.and)
    (if (then
      (call $print (global.get $bullet.ptr) (global.get $bullet.len))
      (call $print (global.get $tls.ptr) (global.get $tls.len))
      (call $print (global.get $newline.ptr) (global.get $newline.len))))
    (local.get $mask) (i32.const 4) (i32.and)
    (if (then
      (call $print (global.get $bullet.ptr) (global.get $bullet.len))
      (call $print (global.get $metrics.ptr) (global.get $metrics.len))
      (call $print (global.get $newline.ptr) (global.get $newline.len))))
    ;; --- confirm ---
    (call $print (global.get $ask-continue.ptr) (global.get $ask-continue.len))
    (local.set $n (call $read_line))
    (local.get $n) (i32.const 0) (i32.lt_s)
    (if (then (call $abort (global.get $input-closed.ptr) (global.get $input-closed.len) (i32.const 2))))
    ;; yes = first byte y/Y; empty or anything else = no
    (local.set $v (i32.const 0))
    (local.get $n) (i32.const 0) (i32.gt_u)
    (if (then
      (i32.load8_u (i32.const 0x100)) (i32.const 121) (i32.eq)
      (i32.load8_u (i32.const 0x100)) (i32.const 89) (i32.eq)
      (i32.or)
      (if (then (local.set $v (i32.const 1))))))
    (local.get $v) (i32.eqz)
    (if (then
      (call $print (global.get $cancelled.ptr) (global.get $cancelled.len))
      (call $exit (i32.const 1))
      (unreachable)))
    ;; --- outro summary ---
    (call $print (global.get $done.ptr) (global.get $done.len))
    (call $print (local.get $np) (local.get $nl))
    (call $print (global.get $on.ptr) (global.get $on.len))
    (call $print (local.get $ep) (local.get $el))
    (call $print (global.get $newline.ptr) (global.get $newline.len))
    (i32.const 0))

  ;; @data 0x1000..0x8000
  (data $intro "◆ prompts demo — project setup\n")
  (data $ask-name "◇ Project name? [my-app]: ")
  (data $label-name "◆ name: ")
  (data $ask-env "◇ Environment? (number):\n")
  (data $env-1 "  1) dev\n")
  (data $env-2 "  2) staging\n")
  (data $env-3 "  3) prod\n")
  (data $prompt "> ")
  (data $env-range "  enter 1-3\n")
  (data $label-env "◆ env: ")
  (data $ask-features "◇ Features? (comma-separated numbers):\n")
  (data $feat-1 "  1) workers\n")
  (data $feat-2 "  2) tls\n")
  (data $feat-3 "  3) metrics\n")
  (data $feat-hint "  pick at least one (e.g. 1,3)\n")
  (data $bullet "  + ")
  (data $newline "\n")
  (data $ask-continue "◇ Continue? (y/N): ")
  (data $cancelled "✖ Cancelled.\n")
  (data $done "◆ Done: ")
  (data $on " on ")
  (data $input-closed "input closed, aborting.\n")
  (data $dev "dev")
  (data $staging "staging")
  (data $prod "prod")
  (data $workers "workers")
  (data $tls "tls")
  (data $metrics "metrics")
  (data $default-name "my-app")
  )

  (core instance $app (instantiate $main
    (with "env" (instance $mem))
    (with "wasi" (instance $wasi))))

  ;; `run: func() -> result`: ok is exit 0, err is a failed run.
  (func $run (result (result)) (canon lift (core func $app "run")))
  (instance $run-i (export "run" (func $run)))
  (export "wasi:cli/run@0.2.12" (instance $run-i))
)
