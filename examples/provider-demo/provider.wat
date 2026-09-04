;; A provider component: exports ai-direct:demo/text with shout(string)->string.
(component
  (core module $m
    (memory (export "memory") 1)
    (global $bump (mut i32) (i32.const 0x1000))

    (func $alloc (param $n i32) (result i32)
      (local $p i32)
      (local.set $p (global.get $bump))
      (global.set $bump (i32.add (global.get $bump) (local.get $n)))
      (local.get $p))

    (func (export "cabi_realloc")
      (param i32 i32 i32 i32) (result i32)
      (call $alloc (local.get 3)))

    ;; shout(ptr, len) -> retptr; the return area holds [ptr, len].
    (func (export "shout") (param $ptr i32) (param $len i32) (result i32)
      (local $out i32) (local $i i32) (local $c i32)
      (local.set $out (call $alloc (local.get $len)))
      (block $done
        (loop $up
          (br_if $done (i32.ge_u (local.get $i) (local.get $len)))
          (local.set $c (i32.load8_u (i32.add (local.get $ptr) (local.get $i))))
          (if (i32.and (i32.ge_u (local.get $c) (i32.const 97))
                       (i32.le_u (local.get $c) (i32.const 122)))
            (then (local.set $c (i32.sub (local.get $c) (i32.const 32)))))
          (i32.store8 (i32.add (local.get $out) (local.get $i)) (local.get $c))
          (local.set $i (i32.add (local.get $i) (i32.const 1)))
          (br $up)))
      (i32.store (i32.const 0x800) (local.get $out))
      (i32.store (i32.const 0x804) (local.get $len))
      (i32.const 0x800)))

  (core instance $i (instantiate $m))
  (alias core export $i "memory" (core memory $mem))
  (alias core export $i "cabi_realloc" (core func $realloc))

  (func $shout (param "text" string) (result string)
    (canon lift (core func $i "shout") (memory $mem) (realloc $realloc)))
  (instance $text (export "shout" (func $shout)))
  (export "ai-direct:demo/text" (instance $text))
)
