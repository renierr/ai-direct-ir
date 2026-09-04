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
;; 0x100 input line (256B), 0x200 parked name, 0x1000+ read-only strings,
;; 0x8000+ canonical ABI bump allocation

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
    (export "input-stream" (type $is (sub resource)))
    (export "output-stream" (type $os (sub resource)))
    (type $se (variant (case "last-operation-failed" (own $ie)) (case "closed")))
    (export "stream-error" (type $sexp (eq $se)))
    (export "[method]input-stream.blocking-read"
      (func (param "self" (borrow $is)) (param "len" u64)
            (result (result (list u8) (error $sexp)))))
    (export "[method]output-stream.blocking-write-and-flush"
      (func (param "self" (borrow $os)) (param "contents" (list u8))
            (result (result (error $sexp)))))))
  (alias export $streams "input-stream" (type $istream))
  (alias export $streams "[method]input-stream.blocking-read" (func $read))
  (alias export $streams "output-stream" (type $ostream))
  (alias export $streams "[method]output-stream.blocking-write-and-flush" (func $write))

  (import "wasi:cli/stdin@0.2.12" (instance $stdin
    (export "input-stream" (type (eq $istream)))
    (export "get-stdin" (func (result (own $istream))))))
  (alias export $stdin "get-stdin" (func $get-stdin))

  (import "wasi:cli/stdout@0.2.12" (instance $stdout
    (export "output-stream" (type (eq $ostream)))
    (export "get-stdout" (func (result (own $ostream))))))
  (alias export $stdout "get-stdout" (func $get-stdout))

  (import "wasi:cli/exit@0.2.12" (instance $exit-i
    (export "exit" (func (param "status" (result))))))
  (alias export $exit-i "exit" (func $exit-fn))

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

  (core func $exit-l (canon lower (func $exit-fn)))
  (core func $get-stdin-l (canon lower (func $get-stdin)))
  (core func $get-stdout-l (canon lower (func $get-stdout)))
  (core func $read-l (canon lower (func $read) (memory $memory) (realloc $realloc)))
  (core func $write-l (canon lower (func $write) (memory $memory) (realloc $realloc)))
  (core instance $wasi
    (export "exit" (func $exit-l))
    (export "get-stdin" (func $get-stdin-l))
    (export "get-stdout" (func $get-stdout-l))
    (export "read" (func $read-l))
    (export "write" (func $write-l)))

  ;; --- application logic, ordinary Core WAT -----------------------------
  (core module $main
    (import "env" "memory" (memory 1))
    (import "wasi" "get-stdin" (func $get_stdin (result i32)))
    (import "wasi" "get-stdout" (func $get_stdout (result i32)))
    (import "wasi" "read" (func $read (param i32 i64 i32)))
    (import "wasi" "write" (func $write (param i32 i32 i32 i32)))
    ;; wasi:cli/exit replaces proc_exit: $abort and the cancel path unwind
    ;; from deep inside the prompt flow, where a return cannot reach.
    (import "wasi" "exit" (func $exit (param i32)))

  (func $print (param $p i32) (param $n i32)
    (call $write (call $get_stdout)
      (local.get $p) (local.get $n) (i32.const 0x08)))

  ;; read_one(ptr) -> 0 ok, non-zero error. Bytes read land at 0x0C, so the
  ;; caller reads it exactly as it read the Preview 1 nread.
  (func $read_one (param $ptr i32) (result i32)
    (call $read (call $get_stdin) (i64.const 1) (i32.const 0x10))
    (if (i32.load (i32.const 0x10)) (then (return (i32.const 1))))
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
    (call $print (i32.const 0x1000) (i32.const 35))   ;; intro
    ;; --- text: project name, empty = default ---
    (call $print (i32.const 0x1023) (i32.const 28))
    (local.set $n (call $read_line))
    (local.get $n) (i32.const 0) (i32.lt_s)
    (if (then (call $abort (i32.const 0x113C) (i32.const 25) (i32.const 2))))
    (local.get $n) (i32.eqz)
    (if
      (then
        (local.set $np (i32.const 0x1174))
        (local.set $nl (i32.const 6)))
      (else
        ;; park the name at 0x200: the 0x100 line buffer is reused
        ;; by every later prompt and would clobber it before $exit.
        (memory.copy (i32.const 0x200) (i32.const 0x100) (local.get $n))
        (local.set $np (i32.const 0x200))
        (local.set $nl (local.get $n))))
    (call $print (i32.const 0x103F) (i32.const 10))
    (call $print (local.get $np) (local.get $nl))
    (call $print (i32.const 0x110A) (i32.const 1))
    ;; --- select: environment 1-3 ---
    (call $print (i32.const 0x1049) (i32.const 27))
    (call $print (i32.const 0x1064) (i32.const 9))
    (call $print (i32.const 0x106D) (i32.const 13))
    (call $print (i32.const 0x107A) (i32.const 10))
    (block $envok
      (loop $env
        (call $print (i32.const 0x1084) (i32.const 2))
        (local.set $n (call $read_line))
        (local.get $n) (i32.const 0) (i32.lt_s)
        (if (then
          (call $abort (i32.const 0x113C) (i32.const 25) (i32.const 2))))
        (local.set $v
          (call $parse_uint (i32.const 0x100) (local.get $n)))
        (i32.and
          (i32.ge_u (local.get $v) (i32.const 1))
          (i32.le_u (local.get $v) (i32.const 3)))
        (if
          (then
            (local.set $ep (i32.const 0x1155))
            (local.set $el (i32.const 3))
            (local.get $v) (i32.const 2) (i32.eq)
            (if (then
              (local.set $ep (i32.const 0x1158))
              (local.set $el (i32.const 7))))
            (local.get $v) (i32.const 3) (i32.eq)
            (if (then
              (local.set $ep (i32.const 0x115F))
              (local.set $el (i32.const 4))))
            (br $envok)))
        (call $print (i32.const 0x1086) (i32.const 12))
        (br $env)))
    (call $print (i32.const 0x1092) (i32.const 9))
    (call $print (local.get $ep) (local.get $el))
    (call $print (i32.const 0x110A) (i32.const 1))
    ;; --- multiselect: features, at least one ---
    (call $print (i32.const 0x109B) (i32.const 41))
    (call $print (i32.const 0x10C4) (i32.const 13))
    (call $print (i32.const 0x10D1) (i32.const 9))
    (call $print (i32.const 0x10DA) (i32.const 13))
    (block $fok
      (loop $f
        (call $print (i32.const 0x1084) (i32.const 2))
        (local.set $n (call $read_line))
        (local.get $n) (i32.const 0) (i32.lt_s)
        (if (then
          (call $abort (i32.const 0x113C) (i32.const 25) (i32.const 2))))
        (local.set $mask
          (call $parse_mask (i32.const 0x100) (local.get $n)))
        (i32.and
          (i32.ge_s (local.get $mask) (i32.const 0))
          (i32.ne (local.get $mask) (i32.const 0)))
        (if (then (br $fok)))
        (call $print (i32.const 0x10E7) (i32.const 31))
        (br $f)))
    (local.get $mask) (i32.const 1) (i32.and)
    (if (then
      (call $print (i32.const 0x1106) (i32.const 4))
      (call $print (i32.const 0x1163) (i32.const 7))
      (call $print (i32.const 0x110A) (i32.const 1))))
    (local.get $mask) (i32.const 2) (i32.and)
    (if (then
      (call $print (i32.const 0x1106) (i32.const 4))
      (call $print (i32.const 0x116A) (i32.const 3))
      (call $print (i32.const 0x110A) (i32.const 1))))
    (local.get $mask) (i32.const 4) (i32.and)
    (if (then
      (call $print (i32.const 0x1106) (i32.const 4))
      (call $print (i32.const 0x116D) (i32.const 7))
      (call $print (i32.const 0x110A) (i32.const 1))))
    ;; --- confirm ---
    (call $print (i32.const 0x110B) (i32.const 21))
    (local.set $n (call $read_line))
    (local.get $n) (i32.const 0) (i32.lt_s)
    (if (then (call $abort (i32.const 0x113C) (i32.const 25) (i32.const 2))))
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
      (call $print (i32.const 0x1120) (i32.const 14))
      (call $exit (i32.const 1))
      (unreachable)))
    ;; --- outro summary ---
    (call $print (i32.const 0x112E) (i32.const 10))
    (call $print (local.get $np) (local.get $nl))
    (call $print (i32.const 0x1138) (i32.const 4))
    (call $print (local.get $ep) (local.get $el))
    (call $print (i32.const 0x110A) (i32.const 1))
    (i32.const 0))

  (data (i32.const 0x1000) "◆ prompts demo — project setup\n")
  (data (i32.const 0x1023) "◇ Project name? [my-app]: ")
  (data (i32.const 0x103F) "◆ name: ")
  (data (i32.const 0x1049) "◇ Environment? (number):\n")
  (data (i32.const 0x1064) "  1) dev\n")
  (data (i32.const 0x106D) "  2) staging\n")
  (data (i32.const 0x107A) "  3) prod\n")
  (data (i32.const 0x1084) "> ")
  (data (i32.const 0x1086) "  enter 1-3\n")
  (data (i32.const 0x1092) "◆ env: ")
  (data (i32.const 0x109B) "◇ Features? (comma-separated numbers):\n")
  (data (i32.const 0x10C4) "  1) workers\n")
  (data (i32.const 0x10D1) "  2) tls\n")
  (data (i32.const 0x10DA) "  3) metrics\n")
  (data (i32.const 0x10E7) "  pick at least one (e.g. 1,3)\n")
  (data (i32.const 0x1106) "  + ")
  (data (i32.const 0x110A) "\n")
  (data (i32.const 0x110B) "◇ Continue? (y/N): ")
  (data (i32.const 0x1120) "✖ Cancelled.\n")
  (data (i32.const 0x112E) "◆ Done: ")
  (data (i32.const 0x1138) " on ")
  (data (i32.const 0x113C) "input closed, aborting.\n")
  (data (i32.const 0x1155) "dev")
  (data (i32.const 0x1158) "staging")
  (data (i32.const 0x115F) "prod")
  (data (i32.const 0x1163) "workers")
  (data (i32.const 0x116A) "tls")
  (data (i32.const 0x116D) "metrics")
  (data (i32.const 0x1174) "my-app")
  )

  (core instance $app (instantiate $main
    (with "env" (instance $mem))
    (with "wasi" (instance $wasi))))

  ;; `run: func() -> result`: ok is exit 0, err is a failed run.
  (func $run (result (result)) (canon lift (core func $app "run")))
  (instance $run-i (export "run" (func $run)))
  (export "wasi:cli/run@0.2.12" (instance $run-i))
)
