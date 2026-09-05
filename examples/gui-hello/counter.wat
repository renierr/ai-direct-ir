;; A reusable core module with no host effects, linked into the component
;; beside the application and sharing its one memory.
(core module $counter
  (import "env" "memory" (memory 1))
  (func (export "add-one") (param i32) (result i32)
    (i32.add (local.get 0) (i32.const 1))))
