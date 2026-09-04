(module
  ;; WASI import: fd_write(fd: i32, iovs: i32, iovs_len: i32, nwritten: i32) -> i32 (errno)
  (import "wasi_snapshot_preview1" "fd_write"
    (func $fd_write (param i32 i32 i32 i32) (result i32)))
  (memory 1)
  (export "memory" (memory 0))

  ;; A named segment: host-rs derives $msg.ptr and $msg.len from it, so the
  ;; length is never restated and can never go stale.
  (data $msg (i32.const 8) "hello from AI-direct IR\n")

  (func (export "_start")
    ;; iovec[0] = { buf, len } stored at 0..8
    (i32.store (i32.const 0) (global.get $msg.ptr))
    (i32.store (i32.const 4) (global.get $msg.len))
    ;; fd_write(stdout=1, iovs=0, iovs_len=1, nwritten=32)
    (call $fd_write
      (i32.const 1)
      (i32.const 0)
      (i32.const 1)
      (i32.const 32))
    drop))
