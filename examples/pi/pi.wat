;; pi.wat -- AI-direct IR demo #2: interactive pi calculator.
;;
;; Spec: prompt for a number 0..1000 (fraction digits of pi), validate,
;; compute pi truncated (not rounded) to N fraction digits, print "3.xxx".
;; A WASI 0.2 component: stdin, stdout, and stderr come from wasi:cli, and
;; the entry point is wasi:cli/run.
;;
;; Algorithm: Rabinowitz-Wagon integer spigot (base 10). Exact, no floats.
;;   len = 10*iters/3 + 1, A[i] = 2, iters = N+2 (one guard digit).
;;   Each outer iteration resolves one more digit; the 9/10 carry logic
;;   (nines/predigit, one digit delay) handles 999.../000... runs.
;;   Stream index 0 is a dummy "0", index 1 is "3", rest are fractions.
;;
;; Build: air build examples/pi/pi.toml
;; Run:   echo 10 | air run examples/pi/pi.toml
;;
;; Memory map (2 pages = 128 KiB):
;;   0..7    scratch              16: write result   32: read result
;;   64..127 input buffer
;;   1024..  static strings       4096..  spigot array A (i32 each, max 3341)
;;   20000.. digit stream buffer  22000.. final output line
;;   32768.. canonical ABI bump allocation

(component
  ;; @wasi stdin stdout stderr pages=2

  ;; --- application logic, ordinary Core WAT -----------------------------
  (core module $main
    (import "env" "memory" (memory 2))
    (import "wasi" "get-stdin" (func $get_stdin (result i32)))
    (import "wasi" "get-stdout" (func $get_stdout (result i32)))
    (import "wasi" "get-stderr" (func $get_stderr (result i32)))
    (import "wasi" "read" (func $read (param i32 i64 i32)))
    (import "wasi" "write" (func $write (param i32 i32 i32 i32)))

  (global $out_idx (mut i32) (i32.const 0))

  (data $prompt (i32.const 1024) "Enter digits (0-1000): ")
  (data $invalid (i32.const 1088) "Invalid input: enter a number 0-1000\n")
  (data $three (i32.const 1200) "3\n")

  ;; write_fd(fd, ptr, len): 1 is stdout, anything else is stderr.
  ;; The stream result lands at 16 and is ignored, as the errno was before.
  (func $write_fd (param $fd i32) (param $ptr i32) (param $len i32)
    (call $write
      (if (result i32) (i32.eq (local.get $fd) (i32.const 1))
        (then (call $get_stdout)) (else (call $get_stderr)))
      (local.get $ptr) (local.get $len) (i32.const 16)))

  ;; read_stdin(ptr, len) -> nread. blocking-read returns a host-allocated
  ;; list, so copy it into the caller's buffer and report the length.
  (func $read_stdin (param $ptr i32) (param $len i32) (result i32)
    (local $got i32) (local $i i32)
    (call $read (call $get_stdin) (i64.extend_i32_u (local.get $len))
      (i32.const 32))
    (if (i32.load (i32.const 32)) (then (return (i32.const 0))))
    (local.set $got (i32.load (i32.const 40)))
    (if (i32.gt_u (local.get $got) (local.get $len))
      (then (local.set $got (local.get $len))))
    (block $done
      (loop $copy
        (br_if $done (i32.ge_u (local.get $i) (local.get $got)))
        (i32.store8 (i32.add (local.get $ptr) (local.get $i))
          (i32.load8_u (i32.add (i32.load (i32.const 36)) (local.get $i))))
        (local.set $i (i32.add (local.get $i) (i32.const 1)))
        (br $copy)))
    (local.get $got))

  ;; parse(ptr, len) -> value or -1. Allows leading spaces, requires
  ;; >=1 digit, rejects value > 1000, rejects trailing junk.
  (func $parse (param $ptr i32) (param $len i32) (result i32)
    (local $i i32) (local $val i32) (local $digits i32) (local $b i32)
    (local.set $i (i32.const 0))
    (local.set $val (i32.const 0))
    (local.set $digits (i32.const 0))
    (block $skip_done
      (loop $skip
        (br_if $skip_done (i32.ge_u (local.get $i) (local.get $len)))
        (local.set $b (i32.load8_u (i32.add (local.get $ptr) (local.get $i))))
        (br_if $skip_done (i32.ne (local.get $b) (i32.const 32)))
        (local.set $i (i32.add (local.get $i) (i32.const 1)))
        (br $skip)))
    (block $dig_done
      (loop $dig
        (br_if $dig_done (i32.ge_u (local.get $i) (local.get $len)))
        (local.set $b (i32.load8_u (i32.add (local.get $ptr) (local.get $i))))
        (br_if $dig_done (i32.lt_u (local.get $b) (i32.const 48)))
        (br_if $dig_done (i32.gt_u (local.get $b) (i32.const 57)))
        (local.set $val
          (i32.add (i32.mul (local.get $val) (i32.const 10))
                   (i32.sub (local.get $b) (i32.const 48))))
        (local.set $digits (i32.add (local.get $digits) (i32.const 1)))
        (local.set $i (i32.add (local.get $i) (i32.const 1)))
        (if (i32.gt_u (local.get $val) (i32.const 1000))
          (then (return (i32.const -1))))
        (br $dig)))
    (if (i32.eqz (local.get $digits))
      (then (return (i32.const -1))))
    (block $trail_ok
      (loop $trail
        (br_if $trail_ok (i32.ge_u (local.get $i) (local.get $len)))
        (local.set $b (i32.load8_u (i32.add (local.get $ptr) (local.get $i))))
        (if (i32.and
              (i32.ne (local.get $b) (i32.const 10))
              (i32.and
                (i32.ne (local.get $b) (i32.const 13))
                (i32.and
                  (i32.ne (local.get $b) (i32.const 32))
                  (i32.ne (local.get $b) (i32.const 0)))))
          (then (return (i32.const -1))))
        (local.set $i (i32.add (local.get $i) (i32.const 1)))
        (br $trail)))
    (local.get $val))

  ;; emit(d): append decimal digit (0-9) as ASCII to stream at 20000
  (func $emit (param $d i32)
    (i32.store8
      (i32.add (i32.const 20000) (global.get $out_idx))
      (i32.add (local.get $d) (i32.const 48)))
    (global.set $out_idx (i32.add (global.get $out_idx) (i32.const 1))))

  ;; pi(n): run spigot for n fraction digits (+1 guard). Stream at 20000,
  ;; length in out_idx. Stream[0] = dummy, stream[1] = "3".
  (func $pi (param $n i32)
    (local $len i32) (local $i i32) (local $j i32) (local $iters i32)
    (local $q i32) (local $x i32) (local $d i32)
    (local $nines i32) (local $predigit i32) (local $k i32)
    (local.set $iters (i32.add (local.get $n) (i32.const 2)))
    (local.set $len
      (i32.add
        (i32.div_u (i32.mul (local.get $iters) (i32.const 10)) (i32.const 3))
        (i32.const 1)))
    (local.set $i (i32.const 0))
    (block $init_done
      (loop $init
        (br_if $init_done (i32.ge_u (local.get $i) (local.get $len)))
        (i32.store
          (i32.add (i32.const 4096) (i32.mul (local.get $i) (i32.const 4)))
          (i32.const 2))
        (local.set $i (i32.add (local.get $i) (i32.const 1)))
        (br $init)))
    (global.set $out_idx (i32.const 0))
    (local.set $nines (i32.const 0))
    (local.set $predigit (i32.const 0))
    (local.set $j (i32.const 0))
    (block $outer_done
      (loop $outer
        (br_if $outer_done (i32.ge_u (local.get $j) (local.get $iters)))
        (local.set $q (i32.const 0))
        (local.set $i (local.get $len))
        (block $inner_done
          (loop $inner
            (br_if $inner_done (i32.eqz (local.get $i)))
            (local.set $x
              (i32.add
                (i32.mul
                  (i32.load
                    (i32.add (i32.const 4096)
                      (i32.mul (i32.sub (local.get $i) (i32.const 1)) (i32.const 4))))
                  (i32.const 10))
                (i32.mul (local.get $q) (local.get $i))))
            (local.set $d (i32.sub (i32.mul (local.get $i) (i32.const 2)) (i32.const 1)))
            (i32.store
              (i32.add (i32.const 4096)
                (i32.mul (i32.sub (local.get $i) (i32.const 1)) (i32.const 4)))
              (i32.rem_u (local.get $x) (local.get $d)))
            (local.set $q (i32.div_u (local.get $x) (local.get $d)))
            (local.set $i (i32.sub (local.get $i) (i32.const 1)))
            (br $inner)))
        (i32.store (i32.const 4096) (i32.rem_u (local.get $q) (i32.const 10)))
        (local.set $q (i32.div_u (local.get $q) (i32.const 10)))
        (if (i32.eq (local.get $q) (i32.const 9))
          (then (local.set $nines (i32.add (local.get $nines) (i32.const 1))))
          (else
            (if (i32.eq (local.get $q) (i32.const 10))
              (then
                (call $emit (i32.add (local.get $predigit) (i32.const 1)))
                (local.set $k (i32.const 0))
                (block $z_done
                  (loop $z
                    (br_if $z_done (i32.ge_u (local.get $k) (local.get $nines)))
                    (call $emit (i32.const 0))
                    (local.set $k (i32.add (local.get $k) (i32.const 1)))
                    (br $z)))
                (local.set $predigit (i32.const 0))
                (local.set $nines (i32.const 0)))
              (else
                (call $emit (local.get $predigit))
                (local.set $predigit (local.get $q))
                (local.set $k (i32.const 0))
                (block $n_done
                  (loop $nloop
                    (br_if $n_done (i32.ge_u (local.get $k) (local.get $nines)))
                    (call $emit (i32.const 9))
                    (local.set $k (i32.add (local.get $k) (i32.const 1)))
                    (br $nloop)))
                (local.set $nines (i32.const 0))))))
        (local.set $j (i32.add (local.get $j) (i32.const 1)))
        (br $outer)))
    ;; flush delayed digit + pending nines
    (call $emit (local.get $predigit))
    (local.set $k (i32.const 0))
    (block $f_done
      (loop $f
        (br_if $f_done (i32.ge_u (local.get $k) (local.get $nines)))
        (call $emit (i32.const 9))
        (local.set $k (i32.add (local.get $k) (i32.const 1)))
        (br $f))))

  (func (export "run") (result i32)
    (local $nread i32) (local $n i32) (local $i i32)
    (call $write_fd (i32.const 1) (global.get $prompt.ptr) (global.get $prompt.len))
    (local.set $nread (call $read_stdin (i32.const 64) (i32.const 64)))
    (if (i32.eqz (local.get $nread))
      (then
        (call $write_fd (i32.const 2) (global.get $invalid.ptr) (global.get $invalid.len))
        (return (i32.const 1))))
    (local.set $n (call $parse (i32.const 64) (local.get $nread)))
    (if (i32.lt_s (local.get $n) (i32.const 0))
      (then
        (call $write_fd (i32.const 2) (global.get $invalid.ptr) (global.get $invalid.len))
        (return (i32.const 1))))
    (if (i32.eqz (local.get $n))
      (then
        (call $write_fd (i32.const 1) (global.get $three.ptr) (global.get $three.len))
        (return (i32.const 0))))
    (call $pi (local.get $n))
    ;; final line at 22000: stream[1] + "." + stream[2..2+n) + "\n"
    (i32.store8 (i32.const 22000) (i32.load8_u (i32.const 20001)))
    (i32.store8 (i32.const 22001) (i32.const 46))
    (local.set $i (i32.const 0))
    (block $copy_done
      (loop $copy
        (br_if $copy_done (i32.ge_u (local.get $i) (local.get $n)))
        (i32.store8
          (i32.add (i32.const 22002) (local.get $i))
          (i32.load8_u (i32.add (i32.const 20002) (local.get $i))))
        (local.set $i (i32.add (local.get $i) (i32.const 1)))
        (br $copy)))
    (i32.store8 (i32.add (i32.const 22002) (local.get $n)) (i32.const 10))
    (call $write_fd (i32.const 1) (i32.const 22000) (i32.add (local.get $n) (i32.const 3)))
    (return (i32.const 0)))
  )

  (core instance $app (instantiate $main
    (with "env" (instance $mem))
    (with "wasi" (instance $wasi))))

  ;; `run: func() -> result`: ok is exit 0, err is a failed run.
  (func $run (result (result)) (canon lift (core func $app "run")))
  (instance $run-i (export "run" (func $run)))
  (export "wasi:cli/run@0.2.12" (instance $run-i))
)
