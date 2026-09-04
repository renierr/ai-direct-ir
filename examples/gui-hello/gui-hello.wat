;; Minimal proof of the gui ABI v1. `frame` is called every egui frame.
(module
  (import "env" "memory" (memory 1))
  (import "ui" "label" (func $label (param i32 i32)))
  (import "ui" "button" (func $button (param i32 i32) (result i32)))
  (import "counter" "add_one" (func $add_one (param i32) (result i32)))
  (global $clicks (mut i32) (i32.const 0))

  (func (export "frame")
    (call $label (global.get $title.ptr) (global.get $title.len))
    (if (call $button (global.get $click.ptr) (global.get $click.len))
      (then (global.set $clicks (call $add_one (global.get $clicks)))))
    (call $label (global.get $status.ptr) (global.get $status.len)))

  ;; Named segments: host-rs derives .ptr/.len, so no length is hand-counted.
  (data $title (i32.const 0) "host-rs GUI proof: egui")
  (data $click (i32.const 32) "Click me safely")
  (data $status (i32.const 64) "Click state stays inside WAT"))
