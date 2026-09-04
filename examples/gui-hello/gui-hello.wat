;; Minimal proof of the gui ABI v1. `frame` is called every egui frame.
(module
  (import "env" "memory" (memory 1))
  (import "ui" "label" (func $label (param i32 i32)))
  (import "ui" "button" (func $button (param i32 i32) (result i32)))
  (import "counter" "add_one" (func $add_one (param i32) (result i32)))
  (global $clicks (mut i32) (i32.const 0))

  (func (export "frame")
    (call $label (i32.const 0) (i32.const 23))
    (if (call $button (i32.const 32) (i32.const 15))
      (then (global.set $clicks (call $add_one (global.get $clicks)))))
    (call $label (i32.const 64) (i32.const 28)))

  (data (i32.const 0) "host-rs GUI proof: egui")
  (data (i32.const 32) "Click me safely")
  (data (i32.const 64) "Click state stays inside WAT"))
