;; prompts-raw.wat -- a WASI 0.2 component driving a raw-terminal setup flow.
;;
;; WAT owns the state and the navigation. Two things it cannot own arrive as
;; imported WIT interfaces:
;;
;;   ai-direct:host/term            raw mode, cursor, size, key events
;;   ai-direct:text-width/width     Unicode display columns
;;
;; The first is the harness's own, so `;; @wasi term` generates it from
;; `air/wit/ai-direct-host/host.wit` -- the same file `air` implements. The
;; second is a vendored provider, still declared by hand.
;;
;; The second is why this example exists. Centering the title needs the number
;; of terminal *columns* it occupies, which is neither its byte count (28) nor
;; its character count: the label carries ANSI styling that costs no columns
;; and a `◆` that costs one. That number comes from a vendored provider
;; component wrapping the `unicode-width` crate -- a released package with a
;; WIT contract, a pinned artifact hash and its upstream licences, not a
;; prebuilt Core module linked by sharing raw memory.
;;
;; Arrows move, Space toggles features, Enter advances, Esc cancels.
;;
;; Memory map (2 pages):
;;   0x0100 terminal size, as (columns, rows)
;;   0x0200 write result
;;   0x1000..0x2000 text, packed by `;; @data`
;;   0x8000+ canonical ABI bump allocation

(component
  ;; @wasi stdout exit-with-code term pages=2

  ;; --- the vendored width provider ---------------------------------------
  (import "ai-direct:text-width/width@0.1.0" (instance $w
    (export "columns" (func (param "text" string) (result u32)))))
  (alias export $w "columns" (func $columns))
  ;; `u32` is a flat result, so unlike a `string` this call needs no return
  ;; area. The string travels the other way: `air` lowers it into the
  ;; provider's own memory through the provider's allocator, never this one.
  (core func $columns-l
    (canon lower (func $columns) (memory $memory) (realloc $realloc)))
  (core instance $prov (export "columns" (func $columns-l)))

  ;; --- application logic, ordinary Core WAT -----------------------------
  (core module $main
    (import "env" "memory" (memory 2))
    (import "wasi" "get-stdout" (func $get_stdout (result i32)))
    (import "wasi" "write" (func $write (param i32 i32 i32 i32)))
    (import "wasi" "exit-with-code" (func $exit (param i32)))
    (import "term" "available" (func $available (result i32)))
    (import "term" "enter" (func $enter (result i32)))
    (import "term" "exit" (func $leave))
    (import "term" "clear" (func $clear))
    (import "term" "move-to" (func $move_to (param i32 i32)))
    (import "term" "size" (func $size (param i32)))
    (import "term" "read-key" (func $read_key (result i32)))
    (import "provider" "columns" (func $columns (param i32 i32) (result i32)))

    (global $SIZE i32 (i32.const 0x100))
    (global $RET i32 (i32.const 0x200))
    ;; `get-stdout` hands out an owned stream on every call, so a redraw loop
    ;; that asked per write would leak one handle per line. Ask once.
    (global $out (mut i32) (i32.const -1))

    ;; @data 0x1000..0x2000
    (data $title "\1b[1;36m◆ Project setup\1b[0m")
    (data $help "\1b[2mArrows move · Space toggles · Enter selects\1b[0m")
    (data $ask_env "\1b[1mChoose an environment\1b[0m")
    (data $ask_feature "\1b[1mSelect one or more features\1b[0m")
    (data $need_feature "\1b[2mSelect at least one feature\1b[0m")
    (data $ask_confirm "\1b[1mCreate this project?\1b[0m")
    (data $dev "dev")
    (data $staging "staging")
    (data $prod "prod")
    (data $workers "workers")
    (data $tls "tls")
    (data $metrics "metrics")
    (data $yes "Yes, create it")
    (data $no "No, cancel setup")
    (data $created "◆ Created configuration for ")
    (data $cancelled "Cancelled.\n")
    (data $features " | features ")
    (data $comma ", ")
    (data $nl "\n")
    (data $need_tty "interactive terminal required; use examples/prompts for pipes\n")
    (data $too_small "terminal must be at least 46 columns x 14 rows\n")
    ;; The four option prefixes: plain, checked, selected, selected+checked.
    (data $unselected "    ")
    (data $checked "\1b[32m[x] ")
    (data $selected "\1b[1;36m> ")
    (data $selected_checked "\1b[1;36m> [x] ")
    (data $reset "\1b[0m")

    (func $p (param $ptr i32) (param $len i32)
      (call $write (global.get $out)
        (local.get $ptr) (local.get $len) (global.get $RET)))
    (func $at (param $x i32) (param $y i32) (param $ptr i32) (param $len i32)
      (call $move_to (local.get $x) (local.get $y))
      (call $p (local.get $ptr) (local.get $len)))

    ;; The prefix for one option row. Multi-value keeps the choice in one
    ;; place: a caller never learns which segment it drew.
    (func $prefix (param $selected i32) (param $checked i32) (result i32 i32)
      (if (local.get $selected)
        (then
          (if (local.get $checked)
            (then (return (global.get $selected_checked.ptr)
                          (global.get $selected_checked.len)))
            (else (return (global.get $selected.ptr)
                          (global.get $selected.len))))))
      (if (local.get $checked)
        (then (return (global.get $checked.ptr) (global.get $checked.len))))
      (global.get $unselected.ptr) (global.get $unselected.len))

    (func $opt (param $x i32) (param $y i32)
      (param $selected i32) (param $checked i32)
      (param $label i32) (param $len i32)
      (local $ptr i32) (local $plen i32)
      (call $prefix (local.get $selected) (local.get $checked))
      (local.set $plen) (local.set $ptr)
      (call $at (local.get $x) (local.get $y) (local.get $ptr) (local.get $plen))
      (call $p (local.get $label) (local.get $len))
      (call $p (global.get $reset.ptr) (global.get $reset.len)))

    ;; The label of environment $env, as (ptr, len).
    (func $env_label (param $env i32) (result i32 i32)
      (if (i32.eqz (local.get $env))
        (then (return (global.get $dev.ptr) (global.get $dev.len))))
      (if (i32.eq (local.get $env) (i32.const 1))
        (then (return (global.get $staging.ptr) (global.get $staging.len))))
      (global.get $prod.ptr) (global.get $prod.len))

    ;; phase: 0 environment, 1 features, 2 confirm. cursor is 0..2 except in
    ;; confirm, where it is 0..1. mask is the three feature bits.
    (func $draw (param $phase i32) (param $cur i32) (param $mask i32)
      (param $x i32) (param $y i32) (param $title_x i32)
      (call $clear)
      (call $at (local.get $title_x) (local.get $y)
        (global.get $title.ptr) (global.get $title.len))
      (call $at (local.get $x) (i32.add (local.get $y) (i32.const 2))
        (global.get $help.ptr) (global.get $help.len))
      (if (i32.eqz (local.get $phase))
        (then
          (call $at (local.get $x) (i32.add (local.get $y) (i32.const 4))
            (global.get $ask_env.ptr) (global.get $ask_env.len))
          (call $opt (local.get $x) (i32.add (local.get $y) (i32.const 6))
            (i32.eqz (local.get $cur)) (i32.const 0)
            (global.get $dev.ptr) (global.get $dev.len))
          (call $opt (local.get $x) (i32.add (local.get $y) (i32.const 7))
            (i32.eq (local.get $cur) (i32.const 1)) (i32.const 0)
            (global.get $staging.ptr) (global.get $staging.len))
          (call $opt (local.get $x) (i32.add (local.get $y) (i32.const 8))
            (i32.eq (local.get $cur) (i32.const 2)) (i32.const 0)
            (global.get $prod.ptr) (global.get $prod.len))))
      (if (i32.eq (local.get $phase) (i32.const 1))
        (then
          (call $at (local.get $x) (i32.add (local.get $y) (i32.const 4))
            (global.get $ask_feature.ptr) (global.get $ask_feature.len))
          (call $opt (local.get $x) (i32.add (local.get $y) (i32.const 6))
            (i32.eqz (local.get $cur)) (i32.and (local.get $mask) (i32.const 1))
            (global.get $workers.ptr) (global.get $workers.len))
          (call $opt (local.get $x) (i32.add (local.get $y) (i32.const 7))
            (i32.eq (local.get $cur) (i32.const 1))
            (i32.and (local.get $mask) (i32.const 2))
            (global.get $tls.ptr) (global.get $tls.len))
          (call $opt (local.get $x) (i32.add (local.get $y) (i32.const 8))
            (i32.eq (local.get $cur) (i32.const 2))
            (i32.and (local.get $mask) (i32.const 4))
            (global.get $metrics.ptr) (global.get $metrics.len))
          (call $at (local.get $x) (i32.add (local.get $y) (i32.const 11))
            (global.get $need_feature.ptr) (global.get $need_feature.len))))
      (if (i32.eq (local.get $phase) (i32.const 2))
        (then
          (call $at (local.get $x) (i32.add (local.get $y) (i32.const 4))
            (global.get $ask_confirm.ptr) (global.get $ask_confirm.len))
          (call $opt (local.get $x) (i32.add (local.get $y) (i32.const 6))
            (i32.eqz (local.get $cur)) (i32.const 0)
            (global.get $yes.ptr) (global.get $yes.len))
          (call $opt (local.get $x) (i32.add (local.get $y) (i32.const 7))
            (i32.eq (local.get $cur) (i32.const 1)) (i32.const 0)
            (global.get $no.ptr) (global.get $no.len)))))

    (func $cancel
      (call $leave)
      (call $p (global.get $cancelled.ptr) (global.get $cancelled.len))
      (call $exit (i32.const 1)) (unreachable))

    (func $done (param $env i32) (param $mask i32)
      (local $ptr i32) (local $len i32)
      (call $leave)
      (call $p (global.get $created.ptr) (global.get $created.len))
      (call $env_label (local.get $env))
      (local.set $len) (local.set $ptr)
      (call $p (local.get $ptr) (local.get $len))
      (call $p (global.get $features.ptr) (global.get $features.len))
      (if (i32.and (local.get $mask) (i32.const 1))
        (then
          (call $p (global.get $workers.ptr) (global.get $workers.len))
          (if (i32.gt_u (local.get $mask) (i32.const 1))
            (then (call $p (global.get $comma.ptr) (global.get $comma.len))))))
      (if (i32.and (local.get $mask) (i32.const 2))
        (then
          (call $p (global.get $tls.ptr) (global.get $tls.len))
          (if (i32.and (local.get $mask) (i32.const 4))
            (then (call $p (global.get $comma.ptr) (global.get $comma.len))))))
      (if (i32.and (local.get $mask) (i32.const 4))
        (then (call $p (global.get $metrics.ptr) (global.get $metrics.len))))
      (call $p (global.get $nl.ptr) (global.get $nl.len))
      (call $exit (i32.const 0)) (unreachable))

    ;; Every path out leaves through `exit-with-code`, so the `result` this
    ;; declares is never produced: the trailing `unreachable` is the whole
    ;; return path, and it satisfies any result type.
    (func (export "run") (result i32)
      (local $cols i32) (local $rows i32)
      (local $x i32) (local $y i32) (local $tx i32)
      (local $phase i32) (local $cur i32) (local $mask i32)
      (local $env i32) (local $k i32) (local $wrap i32)
      (global.set $out (call $get_stdout))
      (if (i32.eqz (call $available))
        (then
          (call $p (global.get $need_tty.ptr) (global.get $need_tty.len))
          (call $exit (i32.const 2)) (unreachable)))
      (if (i32.eqz (call $enter))
        (then (call $exit (i32.const 2)) (unreachable)))
      (call $size (global.get $SIZE))
      (local.set $cols (i32.load (global.get $SIZE)))
      (local.set $rows (i32.load (i32.add (global.get $SIZE) (i32.const 4))))
      (if (i32.or (i32.lt_u (local.get $cols) (i32.const 46))
                  (i32.lt_u (local.get $rows) (i32.const 14)))
        (then
          (call $clear)
          (call $at (i32.const 0) (i32.const 0)
            (global.get $too_small.ptr) (global.get $too_small.len))
          (call $leave)
          (call $exit (i32.const 2)) (unreachable)))
      (local.set $x (i32.shr_u (i32.sub (local.get $cols) (i32.const 40))
                      (i32.const 1)))
      (local.set $y (i32.shr_u (i32.sub (local.get $rows) (i32.const 14))
                      (i32.const 1)))
      ;; The provider answers the title's real column count: its ANSI styling
      ;; costs nothing and its `◆` costs one, so neither the 28 bytes nor the
      ;; 24 characters would center it.
      (local.set $tx
        (i32.shr_u
          (i32.sub (local.get $cols)
            (call $columns (global.get $title.ptr) (global.get $title.len)))
          (i32.const 1)))
      (loop $input
        (call $draw (local.get $phase) (local.get $cur) (local.get $mask)
          (local.get $x) (local.get $y) (local.get $tx))
        (local.set $k (call $read_key))
        ;; Confirm offers two rows; every other phase offers three.
        (local.set $wrap
          (if (result i32) (i32.eq (local.get $phase) (i32.const 2))
            (then (i32.const 2)) (else (i32.const 3))))
        ;; Esc or Ctrl-C.
        (if (i32.or (i32.eq (local.get $k) (i32.const 0x11b))
                    (i32.eq (local.get $k) (i32.const 3)))
          (then (call $cancel)))
        ;; Down.
        (if (i32.eq (local.get $k) (i32.const 0x102))
          (then
            (local.set $cur
              (i32.rem_u (i32.add (local.get $cur) (i32.const 1))
                (local.get $wrap)))
            (br $input)))
        ;; Up: one short of a full turn, so the remainder stays non-negative.
        (if (i32.eq (local.get $k) (i32.const 0x101))
          (then
            (local.set $cur
              (i32.rem_u
                (i32.add (local.get $cur)
                  (i32.sub (local.get $wrap) (i32.const 1)))
                (local.get $wrap)))
            (br $input)))
        ;; Space toggles a feature bit, and only in the feature phase.
        (if (i32.and (i32.eq (local.get $phase) (i32.const 1))
                     (i32.eq (local.get $k) (i32.const 32)))
          (then
            (local.set $mask
              (i32.xor (local.get $mask)
                (i32.shl (i32.const 1) (local.get $cur))))
            (br $input)))
        ;; Enter advances, and in confirm either finishes or cancels.
        (if (i32.eq (local.get $k) (i32.const 0x10d))
          (then
            (if (i32.eqz (local.get $phase))
              (then
                (local.set $env (local.get $cur))
                (local.set $phase (i32.const 1))
                (local.set $cur (i32.const 0))
                (br $input)))
            (if (i32.eq (local.get $phase) (i32.const 1))
              (then
                ;; At least one feature, or the phase does not advance.
                (br_if $input (i32.eqz (local.get $mask)))
                (local.set $phase (i32.const 2))
                (local.set $cur (i32.const 0))
                (br $input)))
            (if (i32.eqz (local.get $cur))
              (then (call $done (local.get $env) (local.get $mask))))
            (call $cancel)))
        (br $input))
      (unreachable)))

  (core instance $app (instantiate $main
    (with "env" (instance $mem))
    (with "wasi" (instance $wasi))
    (with "term" (instance $term))
    (with "provider" (instance $prov))))

  (func $run (result (result)) (canon lift (core func $app "run")))
  (instance $run-i (export "run" (func $run)))
  (export "wasi:cli/run@0.2.12" (instance $run-i))
)
