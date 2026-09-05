(component
  ;; @wasi stdout pages=4 heap=0x20000
  ;; @data 0x1000..0x2000

  (import "wasi:filesystem/types@0.2.12" (instance $types
    (export "descriptor" (type $d (sub resource)))
    (export "output-stream" (type (eq $ostream)))
    (type $ec (enum
      "access" "would-block" "already" "bad-descriptor" "busy" "deadlock"
      "quota" "exist" "file-too-large" "illegal-byte-sequence" "in-progress"
      "interrupted" "invalid" "io" "is-directory" "loop" "too-many-links"
      "message-size" "name-too-long" "no-device" "no-entry" "no-lock"
      "insufficient-memory" "insufficient-space" "not-directory" "not-empty"
      "not-recoverable" "unsupported" "no-tty" "no-such-device" "overflow"
      "not-permitted" "pipe" "read-only" "invalid-seek" "text-file-busy"
      "cross-device"))
    (export "error-code" (type $ecx (eq $ec)))
    (type $pf (flags "symlink-follow"))
    (export "path-flags" (type $pfx (eq $pf)))
    (type $of (flags "create" "directory" "exclusive" "truncate"))
    (export "open-flags" (type $ofx (eq $of)))
    (type $df (flags "read" "write" "file-integrity-sync" "data-integrity-sync"
                     "requested-write-sync" "mutate-directory"))
    (export "descriptor-flags" (type $dfx (eq $df)))
    (export "[method]descriptor.open-at"
      (func (param "self" (borrow $d)) (param "path-flags" $pfx)
            (param "path" string) (param "open-flags" $ofx) (param "flags" $dfx)
            (result (result (own $d) (error $ecx)))))
    (export "[method]descriptor.write-via-stream"
      (func (param "self" (borrow $d)) (param "offset" u64)
            (result (result (own $ostream) (error $ecx)))))))
  (alias export $types "descriptor" (type $descriptor))
  (alias export $types "[method]descriptor.open-at" (func $open-at))
  (alias export $types "[method]descriptor.write-via-stream" (func $wvs))

  (import "wasi:filesystem/preopens@0.2.12" (instance $pre
    (export "descriptor" (type (eq $descriptor)))
    (export "get-directories"
      (func (result (list (tuple (own $descriptor) string)))))))
  (alias export $pre "get-directories" (func $get-dirs))

  (core func $open-at-l (canon lower (func $open-at) (memory $memory) (realloc $realloc)))
  (core func $wvs-l (canon lower (func $wvs) (memory $memory) (realloc $realloc)))
  (core func $get-dirs-l (canon lower (func $get-dirs) (memory $memory) (realloc $realloc)))
  (core instance $fs
    (export "open-at" (func $open-at-l))
    (export "write-via-stream" (func $wvs-l))
    (export "get-directories" (func $get-dirs-l)))

  (core module $main
    (import "env" "memory" (memory 4))
    (import "wasi" "get-stdout" (func $get_stdout (result i32)))
    (import "wasi" "write" (func $write (param i32 i32 i32 i32)))
    (import "fs" "get-directories" (func $get_dirs (param i32)))
    (import "fs" "open-at" (func $open_at (param i32 i32 i32 i32 i32 i32 i32)))
    (import "fs" "write-via-stream" (func $wvs (param i32 i64 i32)))

    (data $name "out.txt")
    (data $body "written by a component\n")
    (data $ok "created out.txt\n")
    (data $denied "open failed (read-only grant?)\n")

    (func $print (param $p i32) (param $n i32)
      (call $write (call $get_stdout) (local.get $p) (local.get $n) (i32.const 0x200)))

    (func (export "run") (result i32)
      (local $desc i32) (local $file i32) (local $stream i32)
      (call $get_dirs (i32.const 0x600))
      (local.set $desc (i32.load (i32.load (i32.const 0x600))))
      ;; open-at(self, path-flags=0, name, open-flags=create(1), flags=write(2))
      (call $open_at (local.get $desc) (i32.const 0)
        (global.get $name.ptr) (global.get $name.len)
        (i32.const 1) (i32.const 2) (i32.const 0x700))
      (if (i32.load8_u (i32.const 0x700))
        (then
          (call $print (global.get $denied.ptr) (global.get $denied.len))
          (return (i32.const 1))))
      (local.set $file (i32.load (i32.const 0x704)))
      (call $wvs (local.get $file) (i64.const 0) (i32.const 0x800))
      (if (i32.load8_u (i32.const 0x800))
        (then
          (call $print (global.get $denied.ptr) (global.get $denied.len))
          (return (i32.const 1))))
      (local.set $stream (i32.load (i32.const 0x804)))
      (call $write (local.get $stream)
        (global.get $body.ptr) (global.get $body.len) (i32.const 0x200))
      (call $print (global.get $ok.ptr) (global.get $ok.len))
      (i32.const 0)))

  (core instance $app (instantiate $main
    (with "env" (instance $mem)) (with "wasi" (instance $wasi)) (with "fs" (instance $fs))))
  (func $run (result (result)) (canon lift (core func $app "run")))
  (instance $r (export "run" (func $run)))
  (export "wasi:cli/run@0.2.12" (instance $r)))
