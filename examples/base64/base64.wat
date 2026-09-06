;; RFC 4648 Base64 through a locked ai-direct:base64 provider.
;; Memory map: 0x100 input, 0x200 stream result, 0x300 return area.
(component
  ;; @wasi stdout
  (import "ai-direct:base64/codec@0.1.0" (instance $codec
    (export "encode" (func (param "text" string) (result string)))))
  (alias export $codec "encode" (func $encode))
  (core func $encode-l
    (canon lower (func $encode) (memory $memory) (realloc $realloc)))
  (core instance $provider (export "encode" (func $encode-l)))
  (core module $main
    (import "env" "memory" (memory 1))
    (import "wasi" "get-stdout" (func $stdout (result i32)))
    (import "wasi" "write" (func $write (param i32 i32 i32 i32)))
    (import "provider" "encode" (func $encode (param i32 i32 i32)))
    ;; @data 0x100..0x200
    (data $input "foobar")
    (func (export "run") (result i32)
      (call $encode (global.get $input.ptr) (global.get $input.len) (i32.const 0x300))
      (call $write (call $stdout) (i32.load (i32.const 0x300))
        (i32.load (i32.const 0x304)) (i32.const 0x200))
      (i32.load (i32.const 0x200))))
  (core instance $app (instantiate $main
    (with "env" (instance $mem))
    (with "wasi" (instance $wasi))
    (with "provider" (instance $provider))))
  (func $run (result (result)) (canon lift (core func $app "run")))
  (instance $run-i (export "run" (func $run)))
  (export "wasi:cli/run@0.2.12" (instance $run-i))
)
