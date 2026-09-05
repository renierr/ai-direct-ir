;; gui-hello.wat -- a WASI 0.2 component drawn by air's egui runtime.
;;
;; A GUI app is an ordinary component. `mode = "gui"` in the manifest decides
;; only who calls the entry point and how often: once per drawn frame instead
;; of once. Nothing else about the linking, the boundary or the grants differs
;; from a command.
;;
;; The host's UI capability arrives as a WIT interface, not a Core namespace:
;;
;;   label:  func(text: string)
;;   button: func(text: string) -> bool
;;
;; so a label crosses by value. The canonical ABI does the copy, the bounds
;; check and the UTF-8 validation that the retired `ui.*` pointer calls had to
;; do by hand in the host.
;;
;; `;; @wasi ui` generates that boundary from `air/wit/ai-direct-host/host.wit`,
;; the same file `air` implements the interface against. No WASI interface is
;; imported at all: drawing is the whole program.
;;
;; Memory map (1 page):
;;   0x100..0x200 text, packed by `;; @data`
;;   0x200        the status line, re-rendered every frame
;;   0x8000+      canonical ABI bump allocation

(component
  ;; @wasi ui pages=1

  ;; A second core module, linked here and sharing the one memory. This is
  ;; what `[[libs]]` did for Core apps; inside a component the same split is
  ;; core instantiation, and it needs no manifest entry.
  ;; @include counter.wat
  (core instance $count (instantiate $counter (with "env" (instance $mem))))

  ;; --- application logic, ordinary Core WAT -----------------------------
  (core module $main
    (import "env" "memory" (memory 1))
    (import "ui" "label" (func $label (param i32 i32)))
    ;; `bool` is an `i32` once lowered: 0 or 1.
    (import "ui" "button" (func $button (param i32 i32) (result i32)))
    (import "counter" "add-one" (func $add_one (param i32) (result i32)))

    ;; The click count lives here and nowhere else. The host holds no
    ;; application state: it replays what this frame asked to be drawn and
    ;; reports which button the user pressed on the frame before.
    (global $clicks (mut i32) (i32.const 0))
    (global $LINE i32 (i32.const 0x200))

    ;; @data 0x100..0x200
    (data $title "AIR GUI proof: egui")
    (data $click "Click me safely")
    (data $status "Clicks: ")

    ;; Write $n as decimal at $at and answer the address just past the last
    ;; digit. Recursive, so the more significant digits are written first.
    (func $digits (param $at i32) (param $n i32) (result i32)
      (local $end i32)
      (local.set $end
        (if (result i32) (i32.ge_u (local.get $n) (i32.const 10))
          (then (call $digits (local.get $at)
                  (i32.div_u (local.get $n) (i32.const 10))))
          (else (local.get $at))))
      (i32.store8 (local.get $end)
        (i32.add (i32.const 48) (i32.rem_u (local.get $n) (i32.const 10))))
      (i32.add (local.get $end) (i32.const 1)))

    ;; Render "Clicks: <n>" at $LINE and answer its length.
    (func $status_line (result i32)
      (memory.copy (global.get $LINE)
        (global.get $status.ptr) (global.get $status.len))
      (i32.sub
        (call $digits
          (i32.add (global.get $LINE) (global.get $status.len))
          (global.get $clicks))
        (global.get $LINE)))

    ;; One frame: describe the whole window, then return. The host renders
    ;; afterwards, so a trap here loses the frame rather than the window.
    (func (export "frame")
      (call $label (global.get $title.ptr) (global.get $title.len))
      (if (call $button (global.get $click.ptr) (global.get $click.len))
        (then (global.set $clicks (call $add_one (global.get $clicks)))))
      (call $label (global.get $LINE) (call $status_line))))

  (core instance $app (instantiate $main
    (with "env" (instance $mem))
    (with "ui" (instance $ui))
    (with "counter" (instance $count))))

  (func $frame (canon lift (core func $app "frame")))
  (export "frame" (func $frame))
)
