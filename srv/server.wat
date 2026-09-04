;; srv/server.wat — static file server. All HTTP logic lives here + lib/.
;;
;; Imports:
;;   env.memory (re-exported as "memory" for WASI), net.listen/accept/recv/close
;;   (host TCP syscalls — swappable scaffolding, see docs/17-static-server.md),
;;   lib.* (response helpers), WASI file calls for serving www/.
;; Assumes the single preopened dir (www/) is WASI fd 3.
;; Memory map (page 0, app-owned): see $layout comment in code.

(module
  (import "env" "memory" (memory 2))
  (export "memory" (memory 0))

  (import "net" "listen" (func $listen (param i32) (result i32)))
  (import "net" "accept" (func $accept (param i32) (result i32)))
  (import "net" "recv" (func $recv (param i32 i32 i32) (result i32)))
  (import "net" "close" (func $close (param i32) (result i32)))

  (import "lib" "send_all" (func $send_all (param i32 i32 i32) (result i32)))
  (import "lib" "send_status" (func $send_status (param i32 i32) (result i32)))
  (import "lib" "send_header"
    (func $send_header (param i32 i32 i32 i32 i32) (result i32)))
  (import "lib" "send_crlf" (func $send_crlf (param i32) (result i32)))
  (import "lib" "send_clen" (func $send_clen (param i32 i64) (result i32)))
  (import "lib" "mime_for"
    (func $mime_for (param i32 i32) (result i32 i32)))
  (import "lib" "parse_request"
    (func $parse_request (param i32 i32 i32 i32 i32) (result i32)))

  (import "wasi_snapshot_preview1" "path_open"
    (func $path_open (param i32 i32 i32 i32 i32 i64 i64 i32 i32)
      (result i32)))
  (import "wasi_snapshot_preview1" "fd_read"
    (func $fd_read (param i32 i32 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "fd_filestat_get"
    (func $filestat (param i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "fd_close"
    (func $fd_close (param i32) (result i32)))

  ;; $layout: 0x100 iov, 0x108 nread, 0x110 m_out, 0x114 p_out, 0x118 pl_out,
  ;; 0x11C opened_fd,
  ;; 0x200 filestat(64B), 0x1000 reqbuf(8K), 0x4000 pathbuf(512B),
  ;; 0x8000 chunk(16K), 0xD000 app data. Lib owns 0x10000+.

  ;; --- 1 if path contains "..", else 0 ---
  (func $has_dotdot (param $p i32) (param $n i32) (result i32)
    (local $i i32)
    (local.set $i (i32.const 0))
    (loop $l
      (local.get $i) (i32.const 1) (i32.add) (local.get $n) (i32.ge_u)
      (if (then (i32.const 0) (return)))
      (i32.load8_u (i32.add (local.get $p) (local.get $i)))
      (i32.const 46) (i32.eq)
      (i32.load8_u (i32.add (i32.add (local.get $p) (local.get $i))
        (i32.const 1))) (i32.const 46) (i32.eq)
      (i32.and)
      (if (then (i32.const 1) (return)))
      (local.set $i (i32.add (local.get $i) (i32.const 1)))
      (br $l))
    (i32.const 0))

  ;; --- error reply: status + text/plain body + close-headers ---
  (func $err (param $fd i32) (param $code i32)
    (local $bptr i32) (local $blen i32)
    (local.set $bptr (i32.const 0xD043))  ;; default 500
    (local.set $blen (i32.const 26))
    (local.get $code) (i32.const 400) (i32.eq)
    (if (then (local.set $bptr (i32.const 0xD000))
              (local.set $blen (i32.const 16))))
    (local.get $code) (i32.const 403) (i32.eq)
    (if (then (local.set $bptr (i32.const 0xD010))
              (local.set $blen (i32.const 14))))
    (local.get $code) (i32.const 404) (i32.eq)
    (if (then (local.set $bptr (i32.const 0xD01E))
              (local.set $blen (i32.const 14))))
    (local.get $code) (i32.const 405) (i32.eq)
    (if (then (local.set $bptr (i32.const 0xD02C))
              (local.set $blen (i32.const 23))))
    (call $send_status (local.get $fd) (local.get $code)) (drop)
    (call $send_header (local.get $fd)
      (i32.const 0xD060) (i32.const 12)   ;; Content-Type
      (i32.const 0xD085) (i32.const 10))  ;; text/plain
    (drop)
    (call $send_clen (local.get $fd)
      (i64.extend_i32_u (local.get $blen))) (drop)
    (call $send_header (local.get $fd)
      (i32.const 0xD06C) (i32.const 10)   ;; Connection
      (i32.const 0xD076) (i32.const 5))   ;; close
    (drop)
    (call $send_crlf (local.get $fd)) (drop)
    (call $send_all (local.get $fd) (local.get $bptr) (local.get $blen))
    (drop))

  ;; --- serve one connection, then return (caller closes socket) ---
  (func $handle (param $cfd i32)
    (local $n i32) (local $rc i32) (local $method i32)
    (local $p i32) (local $pl i32)
    (local $flen i32) (local $errno i32) (local $ffd i32)
    (local $size i64) (local $mptr i32) (local $mlen i32)
    (local $nread i32)
    ;; read request (single recv; requests bigger than 8K get a 400)
    (local.set $n (call $recv (local.get $cfd)
      (i32.const 0x1000) (i32.const 8192)))
    (local.get $n) (i32.const 0) (i32.le_s) (if (then (return)))
    (local.set $rc (call $parse_request (i32.const 0x1000) (local.get $n)
      (i32.const 0x110) (i32.const 0x114) (i32.const 0x118)))
    (local.get $rc) (i32.const 0) (i32.ne)
    (if (then (call $err (local.get $cfd) (i32.const 400)) (return)))
    (local.set $method (i32.load (i32.const 0x110)))
    (local.set $p (i32.load (i32.const 0x114)))
    (local.set $pl (i32.load (i32.const 0x118)))
    (local.get $method) (i32.const 0) (i32.ne)
    (if (then (call $err (local.get $cfd) (i32.const 405)) (return)))
    (call $has_dotdot (local.get $p) (local.get $pl))
    (if (then (call $err (local.get $cfd) (i32.const 403)) (return)))
    ;; build fs path in 0x4000: "/" -> "index.html", else strip leading "/",
    ;; trailing "/" gains "index.html"
    (local.get $pl) (i32.const 1) (i32.eq)
    (if
      (then
        (memory.copy (i32.const 0x4000) (i32.const 0xD07B) (i32.const 10))
        (local.set $flen (i32.const 10)))
      (else
        (local.set $flen (i32.sub (local.get $pl) (i32.const 1)))
        (local.get $flen) (i32.const 500) (i32.gt_u)
        (if (then (call $err (local.get $cfd) (i32.const 400)) (return)))
        (memory.copy (i32.const 0x4000)
          (i32.add (local.get $p) (i32.const 1)) (local.get $flen))
        (i32.load8_u
          (i32.add (i32.const 0x4000)
            (i32.sub (local.get $flen) (i32.const 1))))
        (i32.const 47) (i32.eq)
        (if (then
          (memory.copy (i32.add (i32.const 0x4000) (local.get $flen))
            (i32.const 0xD07B) (i32.const 10))
          (local.set $flen (i32.add (local.get $flen) (i32.const 10)))))))
    ;; open under preopened www/ (fd 3). 44 = NOENT -> 404.
    (local.set $errno (call $path_open (i32.const 3) (i32.const 0)
      (i32.const 0x4000) (local.get $flen) (i32.const 0)
      (i64.const 2097190) (i64.const 0) (i32.const 0) (i32.const 0x11C)))
    (local.set $ffd (i32.load (i32.const 0x11C)))
    (local.get $errno) (i32.const 44) (i32.eq)
    (if (then (call $err (local.get $cfd) (i32.const 404)) (return)))
    (local.get $errno) (i32.const 0) (i32.ne)
    (if (then (call $err (local.get $cfd) (i32.const 500)) (return)))
    ;; directories are not servable -> 404
    (call $filestat (local.get $ffd) (i32.const 0x200))
    (i32.const 0) (i32.ne)
    (if (then
      (call $fd_close (local.get $ffd)) (drop)
      (call $err (local.get $cfd) (i32.const 500)) (return)))
    (i32.load8_u (i32.const 0x210))  ;; filetype at filestat+16
    (i32.const 4) (i32.ne)           ;; 4 = regular file
    (if (then
      (call $fd_close (local.get $ffd)) (drop)
      (call $err (local.get $cfd) (i32.const 404)) (return)))
    (local.set $size (i64.load offset=32 (i32.const 0x200)))
    ;; headers (MIME from the resolved fs path, so "/" -> html)
    (call $mime_for (i32.const 0x4000) (local.get $flen))
    (local.set $mlen)   ;; 2nd result on top
    (local.set $mptr)   ;; 1st result
    (call $send_status (local.get $cfd) (i32.const 200)) (drop)
    (call $send_header (local.get $cfd)
      (i32.const 0xD060) (i32.const 12)
      (local.get $mptr) (local.get $mlen)) (drop)
    (call $send_clen (local.get $cfd) (local.get $size)) (drop)
    (call $send_header (local.get $cfd)
      (i32.const 0xD06C) (i32.const 10)
      (i32.const 0xD076) (i32.const 5)) (drop)
    (call $send_crlf (local.get $cfd)) (drop)
    ;; body: read chunks, send_all each
    (i32.store (i32.const 0x100) (i32.const 0x8000))
    (i32.store (i32.const 0x104) (i32.const 16384))
    (block $eof
      (loop $rd
        (call $fd_read (local.get $ffd)
          (i32.const 0x100) (i32.const 1) (i32.const 0x108))
        (i32.const 0) (i32.ne)
        (if (then (br $eof)))  ;; read error: truncate, close below
        (local.set $nread (i32.load (i32.const 0x108)))
        (local.get $nread) (i32.eqz)
        (if (then (br $eof)))
        (call $send_all (local.get $cfd)
          (i32.const 0x8000) (local.get $nread)) (drop)
        (br $rd)))
    (call $fd_close (local.get $ffd)) (drop))

  ;; --- entry: listen forever, one connection at a time ---
  (func (export "run") (param $port i32) (result i32)
    (local $l i32) (local $c i32)
    (local.set $l (call $listen (local.get $port)))
    (local.get $l) (i32.const 0) (i32.lt_s)
    (if (then (i32.const -1) (return)))
    (loop $srv
      (local.set $c (call $accept (local.get $l)))
      (local.get $c) (i32.const 0) (i32.ge_s)
      (if (then
        (call $handle (local.get $c))
        (call $close (local.get $c)) (drop)))
      (br $srv))
    (i32.const 0))

  (data (i32.const 0xD000) "400 Bad Request\n")
  (data (i32.const 0xD010) "403 Forbidden\n")
  (data (i32.const 0xD01E) "404 Not Found\n")
  (data (i32.const 0xD02C) "405 Method Not Allowed\n")
  (data (i32.const 0xD043) "500 Internal Server Error\n")
  (data (i32.const 0xD060) "Content-Type")
  (data (i32.const 0xD06C) "Connection")
  (data (i32.const 0xD076) "close")
  (data (i32.const 0xD07B) "index.html")
  (data (i32.const 0xD085) "text/plain")
)
