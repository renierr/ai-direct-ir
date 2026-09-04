;; Reusable shared-memory GUI library proof. It has no host effects.
(module
  (import "env" "memory" (memory 1))
  (func (export "add_one") (param i32) (result i32)
    (i32.add (local.get 0) (i32.const 1))))
