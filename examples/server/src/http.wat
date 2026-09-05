;; http.wat -- response building and request parsing for examples/server.
;;
;; This is what `libs/http/http.wasm` used to be, with one structural change:
;; it builds the whole response in a buffer instead of writing pieces to a
;; socket. The Core lib called `net.send` per header; a component holds an
;; `output-stream` handle, and a handle cannot cross a provider boundary, so
;; the I/O has to stay in the application anyway. One buffered response and
;; one `blocking-write-and-flush` is both simpler and fewer crossings.
;;
;; Everything here is pure: bytes in, bytes in the buffer, no imports. The two
;; genuinely reusable pieces -- `$parse_request` and `$mime_for` -- are the
;; ones that could become a catalog provider, precisely because they touch no
;; handles.
;;
;; Included into the root module, so it shares its memory, its data segments
;; and its `$name.ptr` / `$name.len` globals.

  ;; --- the response buffer ----------------------------------------------
  (global $RESP i32 (i32.const 0x4000))
  (global $RESP_END i32 (i32.const 0x14000))
  (global $rlen (mut i32) (i32.const 0))
  ;; Set once an append did not fit. The caller answers 500 rather than
  ;; sending a truncated response with an honest-looking Content-Length.
  (global $rfull (mut i32) (i32.const 0))

  (func $r_reset
    (global.set $rlen (i32.const 0))
    (global.set $rfull (i32.const 0)))

  (func $r_put (param $p i32) (param $n i32)
    (if (i32.gt_u
          (i32.add (i32.add (global.get $RESP) (global.get $rlen)) (local.get $n))
          (global.get $RESP_END))
      (then (global.set $rfull (i32.const 1)) (return)))
    (memory.copy (i32.add (global.get $RESP) (global.get $rlen))
                 (local.get $p) (local.get $n))
    (global.set $rlen (i32.add (global.get $rlen) (local.get $n))))

  (func $r_byte (param $b i32)
    (if (i32.ge_u (i32.add (global.get $RESP) (global.get $rlen))
                  (global.get $RESP_END))
      (then (global.set $rfull (i32.const 1)) (return)))
    (i32.store8 (i32.add (global.get $RESP) (global.get $rlen)) (local.get $b))
    (global.set $rlen (i32.add (global.get $rlen) (i32.const 1))))

  ;; Decimal, no leading zeros. Recursive: the digit count is bounded by the
  ;; buffer size, so the stack is not a concern.
  (func $r_u32 (param $v i32)
    (if (i32.ge_u (local.get $v) (i32.const 10))
      (then (call $r_u32 (i32.div_u (local.get $v) (i32.const 10)))))
    (call $r_byte
      (i32.add (i32.const 48) (i32.rem_u (local.get $v) (i32.const 10)))))

  (func $r_crlf
    (call $r_put (global.get $crlf.ptr) (global.get $crlf.len)))

  (func $r_header (param $k i32) (param $kl i32) (param $v i32) (param $vl i32)
    (call $r_put (local.get $k) (local.get $kl))
    (call $r_put (global.get $colon.ptr) (global.get $colon.len))
    (call $r_put (local.get $v) (local.get $vl))
    (call $r_crlf))

  (func $r_clen (param $n i32)
    (call $r_put (global.get $h-clen.ptr) (global.get $h-clen.len))
    (call $r_u32 (local.get $n))
    (call $r_crlf))

  ;; --- status lines, whole rather than composed -------------------------
  (func $r_status (param $code i32)
    (local $p i32) (local $n i32)
    (local.set $p (global.get $st-500.ptr))
    (local.set $n (global.get $st-500.len))
    (if (i32.eq (local.get $code) (i32.const 200)) (then
      (local.set $p (global.get $st-200.ptr))
      (local.set $n (global.get $st-200.len))))
    (if (i32.eq (local.get $code) (i32.const 400)) (then
      (local.set $p (global.get $st-400.ptr))
      (local.set $n (global.get $st-400.len))))
    (if (i32.eq (local.get $code) (i32.const 403)) (then
      (local.set $p (global.get $st-403.ptr))
      (local.set $n (global.get $st-403.len))))
    (if (i32.eq (local.get $code) (i32.const 404)) (then
      (local.set $p (global.get $st-404.ptr))
      (local.set $n (global.get $st-404.len))))
    (if (i32.eq (local.get $code) (i32.const 405)) (then
      (local.set $p (global.get $st-405.ptr))
      (local.set $n (global.get $st-405.len))))
    (call $r_put (local.get $p) (local.get $n)))

  ;; The plain-text body that goes with a status code.
  (func $body_for (param $code i32) (result i32 i32)
    (if (i32.eq (local.get $code) (i32.const 400))
      (then (return (global.get $b-400.ptr) (global.get $b-400.len))))
    (if (i32.eq (local.get $code) (i32.const 403))
      (then (return (global.get $b-403.ptr) (global.get $b-403.len))))
    (if (i32.eq (local.get $code) (i32.const 404))
      (then (return (global.get $b-404.ptr) (global.get $b-404.len))))
    (if (i32.eq (local.get $code) (i32.const 405))
      (then (return (global.get $b-405.ptr) (global.get $b-405.len))))
    (global.get $b-500.ptr) (global.get $b-500.len))

  ;; --- a complete response ----------------------------------------------
  ;; headers(code, content-type, body length); the caller appends the body.
  (func $r_head (param $code i32) (param $ct i32) (param $ctl i32)
    (param $blen i32)
    (call $r_reset)
    (call $r_status (local.get $code))
    (call $r_header (global.get $h-ctype.ptr) (global.get $h-ctype.len)
                    (local.get $ct) (local.get $ctl))
    (call $r_clen (local.get $blen))
    (call $r_header (global.get $h-conn.ptr) (global.get $h-conn.len)
                    (global.get $v-close.ptr) (global.get $v-close.len))
    (call $r_crlf))

  ;; A status-code-only reply, headers and body together.
  (func $r_error (param $code i32)
    (local $p i32) (local $n i32)
    (call $body_for (local.get $code))
    (local.set $n) (local.set $p)
    (call $r_head (local.get $code)
                  (global.get $m-text.ptr) (global.get $m-text.len)
                  (local.get $n))
    (call $r_put (local.get $p) (local.get $n)))

  ;; --- pure helpers ------------------------------------------------------
  ;; 1 if [a,a+n) == [b,b+n) else 0.
  (func $eq (param $a i32) (param $b i32) (param $n i32) (result i32)
    (local $i i32)
    (loop $l
      (if (i32.ge_u (local.get $i) (local.get $n))
        (then (return (i32.const 1))))
      (if (i32.ne (i32.load8_u (i32.add (local.get $a) (local.get $i)))
                  (i32.load8_u (i32.add (local.get $b) (local.get $i))))
        (then (return (i32.const 0))))
      (local.set $i (i32.add (local.get $i) (i32.const 1)))
      (br $l))
    (i32.const 1))

  ;; 1 if the path contains "..", else 0.
  (func $has_dotdot (param $p i32) (param $n i32) (result i32)
    (local $i i32)
    (loop $l
      (if (i32.ge_u (i32.add (local.get $i) (i32.const 1)) (local.get $n))
        (then (return (i32.const 0))))
      (if (i32.and
            (i32.eq (i32.load8_u (i32.add (local.get $p) (local.get $i)))
                    (i32.const 46))
            (i32.eq (i32.load8_u (i32.add (i32.add (local.get $p) (local.get $i))
                                          (i32.const 1)))
                    (i32.const 46)))
        (then (return (i32.const 1))))
      (local.set $i (i32.add (local.get $i) (i32.const 1)))
      (br $l))
    (i32.const 0))

  ;; Content-Type for a filesystem path, by extension.
  (func $mime_for (param $path i32) (param $len i32) (result i32 i32)
    (local $i i32) (local $ext i32) (local $extlen i32)
    (local.set $i (i32.sub (local.get $len) (i32.const 1)))
    (local.set $ext (i32.const -1))
    (block $done
      (loop $scan
        (br_if $done (i32.lt_s (local.get $i) (i32.const 0)))
        (br_if $done
          (i32.gt_u (i32.sub (local.get $len) (local.get $i)) (i32.const 8)))
        (if (i32.eq (i32.load8_u (i32.add (local.get $path) (local.get $i)))
                    (i32.const 46))
          (then (local.set $ext (i32.add (local.get $i) (i32.const 1)))
                (br $done)))
        (local.set $i (i32.sub (local.get $i) (i32.const 1)))
        (br $scan)))
    (if (i32.lt_s (local.get $ext) (i32.const 0))
      (then (return (global.get $m-bin.ptr) (global.get $m-bin.len))))
    (local.set $extlen (i32.sub (local.get $len) (local.get $ext)))
    (local.set $ext (i32.add (local.get $path) (local.get $ext)))
    (if (call $ext_is (local.get $ext) (local.get $extlen)
               (global.get $e-html.ptr) (global.get $e-html.len))
      (then (return (global.get $m-html.ptr) (global.get $m-html.len))))
    (if (call $ext_is (local.get $ext) (local.get $extlen)
               (global.get $e-css.ptr) (global.get $e-css.len))
      (then (return (global.get $m-css.ptr) (global.get $m-css.len))))
    (if (call $ext_is (local.get $ext) (local.get $extlen)
               (global.get $e-js.ptr) (global.get $e-js.len))
      (then (return (global.get $m-js.ptr) (global.get $m-js.len))))
    (if (call $ext_is (local.get $ext) (local.get $extlen)
               (global.get $e-json.ptr) (global.get $e-json.len))
      (then (return (global.get $m-json.ptr) (global.get $m-json.len))))
    (if (call $ext_is (local.get $ext) (local.get $extlen)
               (global.get $e-wasm.ptr) (global.get $e-wasm.len))
      (then (return (global.get $m-wasm.ptr) (global.get $m-wasm.len))))
    (if (call $ext_is (local.get $ext) (local.get $extlen)
               (global.get $e-txt.ptr) (global.get $e-txt.len))
      (then (return (global.get $m-text.ptr) (global.get $m-text.len))))
    (global.get $m-bin.ptr) (global.get $m-bin.len))

  (func $ext_is (param $ext i32) (param $extlen i32)
    (param $want i32) (param $wantlen i32) (result i32)
    (i32.and (i32.eq (local.get $extlen) (local.get $wantlen))
             (call $eq (local.get $ext) (local.get $want) (local.get $wantlen))))

  ;; parse_request(buf, len, m_out, p_out, pl_out) -> 0 ok / 1 bad.
  ;; m_out: 0 = GET, 1 = anything else (the caller answers 405 unless it
  ;; recognises the path). The path points into the request buffer.
  (func $parse_request
    (param $buf i32) (param $blen i32)
    (param $m_out i32) (param $p_out i32) (param $pl_out i32)
    (result i32)
    (local $ps i32) (local $sp i32) (local $q i32) (local $end i32)
    (local.set $end (i32.add (local.get $buf) (local.get $blen)))
    (if (i32.lt_u (local.get $blen) (i32.const 14))
      (then (return (i32.const 1))))
    (if (call $eq (local.get $buf) (global.get $m-get.ptr) (i32.const 4))
      (then
        (i32.store (local.get $m_out) (i32.const 0))
        (local.set $ps (i32.add (local.get $buf) (i32.const 4))))
      (else
        (i32.store (local.get $m_out) (i32.const 1))
        (local.set $ps (local.get $buf))
        (block $f
          (loop $s
            (br_if $f (i32.ge_u (local.get $ps) (local.get $end)))
            (br_if $f (i32.eq (i32.load8_u (local.get $ps)) (i32.const 32)))
            (local.set $ps (i32.add (local.get $ps) (i32.const 1)))
            (br $s)))
        (if (i32.ge_u (local.get $ps) (local.get $end))
          (then (return (i32.const 1))))
        (local.set $ps (i32.add (local.get $ps) (i32.const 1)))
        (if (i32.ge_u (local.get $ps) (local.get $end))
          (then (return (i32.const 1))))))
    ;; the path must start with '/'
    (if (i32.ne (i32.load8_u (local.get $ps)) (i32.const 47))
      (then (return (i32.const 1))))
    (local.set $sp (local.get $ps))
    (block $found
      (loop $scan
        (br_if $found (i32.ge_u (local.get $sp) (local.get $end)))
        (br_if $found (i32.eq (i32.load8_u (local.get $sp)) (i32.const 32)))
        (local.set $sp (i32.add (local.get $sp) (i32.const 1)))
        (br $scan)))
    (if (i32.ge_u (local.get $sp) (local.get $end))
      (then (return (i32.const 1))))
    (if (i32.gt_u (i32.add (local.get $sp) (i32.const 6)) (local.get $end))
      (then (return (i32.const 1))))
    (if (i32.eqz (call $eq (i32.add (local.get $sp) (i32.const 1))
                       (global.get $m-http.ptr) (i32.const 5)))
      (then (return (i32.const 1))))
    ;; strip the query string
    (local.set $q (local.get $ps))
    (block $noq
      (loop $qscan
        (br_if $noq (i32.ge_u (local.get $q) (local.get $sp)))
        (br_if $noq (i32.eq (i32.load8_u (local.get $q)) (i32.const 63)))
        (local.set $q (i32.add (local.get $q) (i32.const 1)))
        (br $qscan)))
    (i32.store (local.get $p_out) (local.get $ps))
    (i32.store (local.get $pl_out) (i32.sub (local.get $q) (local.get $ps)))
    (i32.const 0))

  ;; Absolute address just past the first \r\n\r\n, or 0.
  (func $find_body (param $buf i32) (param $n i32) (result i32)
    (local $i i32) (local $end i32)
    (local.set $i (local.get $buf))
    (local.set $end (i32.sub (i32.add (local.get $buf) (local.get $n))
                             (i32.const 3)))
    (loop $s
      (if (i32.ge_u (local.get $i) (local.get $end))
        (then (return (i32.const 0))))
      (if (i32.and
            (i32.eq (i32.load8_u (local.get $i)) (i32.const 13))
            (i32.and
              (i32.eq (i32.load8_u offset=1 (local.get $i)) (i32.const 10))
              (i32.and
                (i32.eq (i32.load8_u offset=2 (local.get $i)) (i32.const 13))
                (i32.eq (i32.load8_u offset=3 (local.get $i)) (i32.const 10)))))
        (then (return (i32.add (local.get $i) (i32.const 4)))))
      (local.set $i (i32.add (local.get $i) (i32.const 1)))
      (br $s))
    (i32.const 0))
