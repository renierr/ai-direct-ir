;; lib/http.wat — "somebody else's lib" for the static-file server.
;;
;; Linking contract (see docs/16-lib-reuse-linking.md):
;; - Memory is owned by the HOST and imported from "env" (min 2 pages).
;;   Both lib and app use it; no cross-module pointers problem because
;;   there is only ONE memory.
;; - 0x00000-0x0FFFF: app-owned. 0x10000-0x17FFF: lib scratch.
;;   0x18000-0x1FFFF: lib read-only data (do not write).
;; - All socket I/O goes through imported net.send (host syscall layer).
;; - Sizes/addresses below are part of the ABI; app must not touch
;;   0x10000 and up.

(module
  (import "env" "memory" (memory 2))
  (import "net" "send"
    (func $net_send (param i32 i32 i32) (result i32)))

  ;; --- byte compare helper: 1 if [a,a+n) == [b,b+n) else 0 ---
  (func $eq (param $a i32) (param $b i32) (param $n i32) (result i32)
    (local $i i32)
    (local.set $i (i32.const 0))
    (loop $l
      (local.get $i) (local.get $n) (i32.ge_u)
      (if (then (i32.const 1) (return)))
      (i32.load8_u (i32.add (local.get $a) (local.get $i)))
      (i32.load8_u (i32.add (local.get $b) (local.get $i)))
      (i32.ne)
      (if (then (i32.const 0) (return)))
      (local.set $i (i32.add (local.get $i) (i32.const 1)))
      (br $l))
    (i32.const 1))

  ;; --- send_all(fd, ptr, len): loop net.send until done. 0 ok, -1 fail ---
  (func $send_all (export "send_all")
    (param $fd i32) (param $ptr i32) (param $len i32) (result i32)
    (local $n i32)
    (loop $l
      (local.get $len) (i32.eqz)
      (if (then (i32.const 0) (return)))
      (local.set $n
        (call $net_send (local.get $fd) (local.get $ptr) (local.get $len)))
      (local.get $n) (i32.const 0) (i32.le_s)
      (if (then (i32.const -1) (return)))
      (local.set $ptr (i32.add (local.get $ptr) (local.get $n)))
      (local.set $len (i32.sub (local.get $len) (local.get $n)))
      (br $l))
    (i32.const 0))

  ;; --- send_status(fd, code): status line for 200/400/403/404/405/500 ---
  (func (export "send_status") (param $fd i32) (param $code i32) (result i32)
    (local $ptr i32) (local $len i32)
    (local.set $ptr (i32.const 0x1807C))  ;; default 500
    (local.set $len (i32.const 36))
    (local.get $code) (i32.const 200) (i32.eq)
    (if (then (local.set $ptr (i32.const 0x18000))
              (local.set $len (i32.const 17))))
    (local.get $code) (i32.const 400) (i32.eq)
    (if (then (local.set $ptr (i32.const 0x18011))
              (local.set $len (i32.const 26))))
    (local.get $code) (i32.const 403) (i32.eq)
    (if (then (local.set $ptr (i32.const 0x1802B))
              (local.set $len (i32.const 24))))
    (local.get $code) (i32.const 404) (i32.eq)
    (if (then (local.set $ptr (i32.const 0x18043))
              (local.set $len (i32.const 24))))
    (local.get $code) (i32.const 405) (i32.eq)
    (if (then (local.set $ptr (i32.const 0x1805B))
              (local.set $len (i32.const 33))))
    (call $send_all (local.get $fd) (local.get $ptr) (local.get $len))
    (drop)
    (i32.const 0))

  ;; --- send_header(fd, kptr,klen, vptr,vlen): "Key: value\r\n" via scratch ---
  (func (export "send_header")
    (param $fd i32) (param $k i32) (param $kl i32)
    (param $v i32) (param $vl i32) (result i32)
    (local $o i32)
    (memory.copy (i32.const 0x10000) (local.get $k) (local.get $kl))
    (local.set $o (i32.add (i32.const 0x10000) (local.get $kl)))
    (memory.copy (local.get $o) (i32.const 0x18150) (i32.const 2))
    (local.set $o (i32.add (local.get $o) (i32.const 2)))
    (memory.copy (local.get $o) (local.get $v) (local.get $vl))
    (local.set $o (i32.add (local.get $o) (local.get $vl)))
    (memory.copy (local.get $o) (i32.const 0x18152) (i32.const 2))
    (local.set $o (i32.add (local.get $o) (i32.const 2)))
    (call $send_all (local.get $fd) (i32.const 0x10000)
      (i32.sub (local.get $o) (i32.const 0x10000)))
    (drop)
    (i32.const 0))

  ;; --- send_crlf(fd): blank line ending the header block ---
  (func (export "send_crlf") (param $fd i32) (result i32)
    (call $send_all (local.get $fd) (i32.const 0x18152) (i32.const 2))
    (drop)
    (i32.const 0))

  ;; --- send_clen(fd, len:i64): "Content-Length: <dec>\r\n" ---
  (func (export "send_clen") (param $fd i32) (param $len i64) (result i32)
    (local $ndig i32) (local $v i64) (local $i i32)
    (memory.copy (i32.const 0x10000) (i32.const 0x18140) (i32.const 16))
    (local.set $ndig (i32.const 0))
    (local.set $v (local.get $len))
    (local.get $v) (i64.eqz)
    (if
      (then
        (i32.store8 (i32.const 0x10100) (i32.const 48))
        (local.set $ndig (i32.const 1)))
      (else
        (loop $ex
          (i32.store8
            (i32.add (i32.const 0x10100) (local.get $ndig))
            (i32.add (i32.const 48)
              (i32.wrap_i64
                (i64.rem_u (local.get $v) (i64.const 10)))))
          (local.set $ndig (i32.add (local.get $ndig) (i32.const 1)))
          (local.set $v (i64.div_u (local.get $v) (i64.const 10)))
          (br_if $ex (i64.ne (local.get $v) (i64.const 0))))))
    (local.set $i (i32.const 0))
    (loop $cp
      (local.get $i) (local.get $ndig) (i32.lt_u)
      (if (then
        (i32.store8
          (i32.add (i32.const 0x10010) (local.get $i))
          (i32.load8_u
            (i32.add (i32.const 0x10100)
              (i32.sub
                (i32.sub (local.get $ndig) (i32.const 1))
                (local.get $i)))))
        (local.set $i (i32.add (local.get $i) (i32.const 1)))
        (br $cp))))
    (memory.copy
      (i32.add (i32.const 0x10010) (local.get $ndig))
      (i32.const 0x18152) (i32.const 2))
    (call $send_all (local.get $fd) (i32.const 0x10000)
      (i32.add (i32.add (i32.const 16) (local.get $ndig)) (i32.const 2)))
    (drop)
    (i32.const 0))

  ;; --- mime_for(path, len) -> (ptr, len): content type by extension ---
  (func (export "mime_for")
    (param $path i32) (param $len i32) (result i32 i32)
    (local $i i32) (local $ext i32) (local $extlen i32)
    (local.set $i (i32.sub (local.get $len) (i32.const 1)))
    (local.set $ext (i32.const -1))
    (block $done
      (loop $scan
        (br_if $done (i32.lt_s (local.get $i) (i32.const 0)))
        (br_if $done
          (i32.gt_u
            (i32.sub (local.get $len) (local.get $i)) (i32.const 8)))
        (i32.load8_u (i32.add (local.get $path) (local.get $i)))
        (i32.const 46) (i32.eq)
        (if (then
          (local.set $ext (i32.add (local.get $i) (i32.const 1)))
          (br $done)))
        (local.set $i (i32.sub (local.get $i) (i32.const 1)))
        (br $scan)))
    (local.get $ext) (i32.const 0) (i32.lt_s)
    (if (then
      (i32.const 0x180FB) (i32.const 24)  ;; application/octet-stream
      (return)))
    (local.set $extlen (i32.sub (local.get $len) (local.get $ext)))
    ;; html
    (i32.and
      (i32.eq (local.get $extlen) (i32.const 4))
      (call $eq (i32.add (local.get $path) (local.get $ext))
        (i32.const 0x18120) (i32.const 4)))
    (if (then (i32.const 0x180A0) (i32.const 24) (return)))
    ;; json
    (i32.and
      (i32.eq (local.get $extlen) (i32.const 4))
      (call $eq (i32.add (local.get $path) (local.get $ext))
        (i32.const 0x18129) (i32.const 4)))
    (if (then (i32.const 0x180D1) (i32.const 16) (return)))
    ;; wasm
    (i32.and
      (i32.eq (local.get $extlen) (i32.const 4))
      (call $eq (i32.add (local.get $path) (local.get $ext))
        (i32.const 0x18130) (i32.const 4)))
    (if (then (i32.const 0x180EB) (i32.const 16) (return)))
    ;; css
    (i32.and
      (i32.eq (local.get $extlen) (i32.const 3))
      (call $eq (i32.add (local.get $path) (local.get $ext))
        (i32.const 0x18124) (i32.const 3)))
    (if (then (i32.const 0x180B9) (i32.const 8) (return)))
    ;; txt
    (i32.and
      (i32.eq (local.get $extlen) (i32.const 3))
      (call $eq (i32.add (local.get $path) (local.get $ext))
        (i32.const 0x1812D) (i32.const 3)))
    (if (then (i32.const 0x180E1) (i32.const 10) (return)))
    ;; js
    (i32.and
      (i32.eq (local.get $extlen) (i32.const 2))
      (call $eq (i32.add (local.get $path) (local.get $ext))
        (i32.const 0x18127) (i32.const 2)))
    (if (then (i32.const 0x180C1) (i32.const 16) (return)))
    (i32.const 0x180FB) (i32.const 24))

  ;; --- parse_request(buf, buflen, m_out, p_out, pl_out) -> 0 ok / 1 bad ---
  ;; m_out: 0 = GET, 1 = anything else (caller answers 405).
  ;; Path is zero-copy: p_out points into the request buffer.
  (func (export "parse_request")
    (param $buf i32) (param $blen i32)
    (param $m_out i32) (param $p_out i32) (param $pl_out i32)
    (result i32)
    (local $ps i32) (local $sp i32) (local $q i32)
    (local.get $blen) (i32.const 14) (i32.lt_u)
    (if (then (i32.const 1) (return)))
    (call $eq (local.get $buf) (i32.const 0x18160) (i32.const 4))
    (i32.eqz)
    (if (then  ;; not GET: still 0, method=1, no path
      (i32.store (local.get $m_out) (i32.const 1))
      (i32.store (local.get $p_out) (i32.const 0))
      (i32.store (local.get $pl_out) (i32.const 0))
      (i32.const 0)
      (return)))
    (i32.store (local.get $m_out) (i32.const 0))
    (local.set $ps (i32.add (local.get $buf) (i32.const 4)))
    ;; path must start with '/'
    (i32.load8_u (local.get $ps)) (i32.const 47) (i32.ne)
    (if (then (i32.const 1) (return)))
    ;; find terminating space
    (local.set $sp (local.get $ps))
    (block $found
      (loop $scan
        (br_if $found
          (i32.ge_u (local.get $sp)
            (i32.add (local.get $buf) (local.get $blen))))
        (i32.load8_u (local.get $sp)) (i32.const 32) (i32.eq)
        (br_if $found)
        (local.set $sp (i32.add (local.get $sp) (i32.const 1)))
        (br $scan)))
    (local.get $sp)
    (i32.add (local.get $buf) (local.get $blen)) (i32.ge_u)
    (if (then (i32.const 1) (return)))
    ;; expect "HTTP/" after the space
    (i32.add (local.get $sp) (i32.const 6))
    (i32.add (local.get $buf) (local.get $blen)) (i32.gt_u)
    (if (then (i32.const 1) (return)))
    (call $eq (i32.add (local.get $sp) (i32.const 1))
      (i32.const 0x18164) (i32.const 5))
    (i32.eqz)
    (if (then (i32.const 1) (return)))
    ;; strip query string
    (local.set $q (local.get $ps))
    (block $noq
      (loop $qscan
        (br_if $noq (i32.ge_u (local.get $q) (local.get $sp)))
        (i32.load8_u (local.get $q)) (i32.const 63) (i32.eq)
        (if (then (local.set $sp (local.get $q)) (br $noq)))
        (local.set $q (i32.add (local.get $q) (i32.const 1)))
        (br $qscan)))
    (i32.store (local.get $p_out) (local.get $ps))
    (i32.store (local.get $pl_out)
      (i32.sub (local.get $sp) (local.get $ps)))
    (i32.const 0))

  ;; --- lib data: status lines, mime types, compare constants ---
  (data (i32.const 0x18000) "HTTP/1.1 200 OK\r\n")
  (data (i32.const 0x18011) "HTTP/1.1 400 Bad Request\r\n")
  (data (i32.const 0x1802B) "HTTP/1.1 403 Forbidden\r\n")
  (data (i32.const 0x18043) "HTTP/1.1 404 Not Found\r\n")
  (data (i32.const 0x1805B) "HTTP/1.1 405 Method Not Allowed\r\n")
  (data (i32.const 0x1807C) "HTTP/1.1 500 Internal Server Error\r\n")
  (data (i32.const 0x180A0) "text/html; charset=utf-8")
  (data (i32.const 0x180B9) "text/css")
  (data (i32.const 0x180C1) "text/javascript")
  (data (i32.const 0x180D1) "application/json")
  (data (i32.const 0x180E1) "text/plain")
  (data (i32.const 0x180EB) "application/wasm")
  (data (i32.const 0x180FB) "application/octet-stream")
  (data (i32.const 0x18120) "html")
  (data (i32.const 0x18124) "css")
  (data (i32.const 0x18127) "js")
  (data (i32.const 0x18129) "json")
  (data (i32.const 0x1812D) "txt")
  (data (i32.const 0x18130) "wasm")
  (data (i32.const 0x18140) "Content-Length: ")
  (data (i32.const 0x18150) ": ")
  (data (i32.const 0x18152) "\r\n")
  (data (i32.const 0x18160) "GET ")
  (data (i32.const 0x18164) "HTTP/")
)
