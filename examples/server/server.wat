;; server.wat -- a static file server as a WASI 0.2 component.
;;
;; It owns its accept loop: `;; @wasi sockets` generates the `wasi:sockets`
;; boundary, `filesystem` generates the `wasi:filesystem` one, and the two
;; `(import "net" ...)` / `(import "fs" ...)` blocks below decide how much of
;; each gets declared. Nothing in this file is a host syscall -- the `net.*`
;; layer this example used to depend on is gone, along with `mode = "server"`.
;;
;; The digest is not here either. `ai-direct:sha256/digest` is the same
;; vendored provider component `examples/sha256sum/` consumes; the Core
;; `[[bridges]]` block that used to memory-copy into a Preview 1 module is
;; replaced by one `[[providers]]` line.
;;
;; Per connection it accepts, reads the request, builds one response in a
;; buffer, writes it once, and gives back every handle it was handed. The
;; drops are load bearing: dropping the accepted socket is what closes the
;; connection, and a file served without dropping its descriptor and stream
;; leaks two handles per request.
;;
;;   air run examples/server/manifest.toml      # :8124
;;   curl -i http://127.0.0.1:8124/
;;   curl -sS --data-binary abc http://127.0.0.1:8124/sha256
;;   curl -sS http://127.0.0.1:8124/quit        # stops the run
;;
;; Memory map (8 pages):
;;   0x0100 accept result   (tag @0, socket @4, input @8, output @12)
;;   0x0120 stream read result      (tag @0, ptr @4, len @8)
;;   0x0140 stream write result
;;   0x0160 preopens result         (ptr @0, count @4)
;;   0x0180 open-at result          (tag @0, descriptor @4)
;;   0x01A0 read-via-stream result  (tag @0, stream @4)
;;   0x01C0 hash-hex result         (ptr @0, len @4)
;;   0x01E0 socket call results     (tag @0, error @1 or @4)
;;   0x0200 filesystem path buffer (512B)
;;   0x0800..0x2000 text, packed by `;; @data`
;;   0x4000..0x14000 response buffer (64K)
;;   0x14000..0x34000 file buffer (128K)
;;   0x40000+ canonical ABI bump allocation

(component
  ;; @wasi stdout stderr exit-with-code sockets filesystem pages=8 heap=0x40000

  ;; --- the digest provider, imported like any other interface ------------
  (import "ai-direct:sha256/digest@0.1.0" (instance $sha
    (export "hash-hex" (func (param "data" (list u8)) (result string)))))
  (alias export $sha "hash-hex" (func $hash-hex))
  (core func $hash-hex-l
    (canon lower (func $hash-hex) (memory $memory) (realloc $realloc)))
  (core instance $prov (export "hash-hex" (func $hash-hex-l)))

  ;; --- application logic, ordinary Core WAT -----------------------------
  (core module $main
    (import "env" "memory" (memory 8))
    (import "env" "heap-mark" (func $heap_mark (result i32)))
    (import "env" "heap-reset" (func $heap_reset (param i32)))

    (import "wasi" "get-stdout" (func $get_stdout (result i32)))
    (import "wasi" "get-stderr" (func $get_stderr (result i32)))
    (import "wasi" "read" (func $read (param i32 i64 i32)))
    (import "wasi" "write" (func $write (param i32 i32 i32 i32)))
    (import "wasi" "exit-with-code" (func $exit (param i32)))
    (import "wasi" "input-stream.drop" (func $drop_in (param i32)))
    (import "wasi" "output-stream.drop" (func $drop_out (param i32)))

    (import "net" "instance-network" (func $network (result i32)))
    (import "net" "create-tcp-socket" (func $create (param i32 i32)))
    (import "net" "tcp-socket.start-bind"
      (func $start_bind
        (param i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32)))
    (import "net" "tcp-socket.finish-bind" (func $finish_bind (param i32 i32)))
    (import "net" "tcp-socket.start-listen" (func $start_listen (param i32 i32)))
    (import "net" "tcp-socket.finish-listen" (func $finish_listen (param i32 i32)))
    (import "net" "tcp-socket.accept" (func $accept (param i32 i32)))
    (import "net" "tcp-socket.subscribe" (func $subscribe (param i32) (result i32)))
    (import "net" "tcp-socket.drop" (func $drop_socket (param i32)))
    (import "net" "pollable.block" (func $block (param i32)))
    (import "net" "pollable.drop" (func $drop_pollable (param i32)))

    (import "fs" "get-directories" (func $get_dirs (param i32)))
    (import "fs" "descriptor.open-at"
      (func $open_at (param i32 i32 i32 i32 i32 i32 i32)))
    (import "fs" "descriptor.read-via-stream"
      (func $read_via_stream (param i32 i64 i32)))
    (import "fs" "descriptor.drop" (func $drop_desc (param i32)))

    (import "provider" "hash-hex" (func $hash_hex (param i32 i32 i32)))

    ;; @data 0x800..0x2000
    (data $crlf "\0d\0a")
    (data $colon ": ")
    (data $h-ctype "Content-Type")
    (data $h-clen "Content-Length: ")
    (data $h-conn "Connection")
    (data $v-close "close")

    (data $st-200 "HTTP/1.1 200 OK\0d\0a")
    (data $st-400 "HTTP/1.1 400 Bad Request\0d\0a")
    (data $st-403 "HTTP/1.1 403 Forbidden\0d\0a")
    (data $st-404 "HTTP/1.1 404 Not Found\0d\0a")
    (data $st-405 "HTTP/1.1 405 Method Not Allowed\0d\0a")
    (data $st-500 "HTTP/1.1 500 Internal Server Error\0d\0a")

    (data $b-400 "400 Bad Request\n")
    (data $b-403 "403 Forbidden\n")
    (data $b-404 "404 Not Found\n")
    (data $b-405 "405 Method Not Allowed\n")
    (data $b-500 "500 Internal Server Error\n")

    (data $m-html "text/html; charset=utf-8")
    (data $m-css "text/css")
    (data $m-js "application/javascript")
    (data $m-json "application/json")
    (data $m-wasm "application/wasm")
    (data $m-text "text/plain; charset=utf-8")
    (data $m-bin "application/octet-stream")
    (data $e-html "html")
    (data $e-css "css")
    (data $e-js "js")
    (data $e-json "json")
    (data $e-wasm "wasm")
    (data $e-txt "txt")
    (data $m-get "GET ")
    (data $m-http "HTTP/")

    (data $p-index "index.html")
    (data $p-sha256 "/sha256")
    (data $p-quit "/quit")
    (data $p-hello "/hello")
    (data $hello-body "hello, air!\n")

    (data $ready "server: listening on 127.0.0.1:8124\n")
    (data $bye "server: /quit\n")
    (data $failed "server: wasi:sockets error-code ")

    ;; @include src/http.wat

    ;; --- the granted directory, opened once at startup -------------------
    (global $dir (mut i32) (i32.const -1))
    (global $FILE i32 (i32.const 0x14000))
    (global $FILE_CAP i32 (i32.const 0x20000))

    (func $print (param $p i32) (param $n i32)
      (call $write (call $get_stdout) (local.get $p) (local.get $n)
        (i32.const 0x140)))

    (func $eprint (param $p i32) (param $n i32)
      (call $write (call $get_stderr) (local.get $p) (local.get $n)
        (i32.const 0x140)))

    ;; Every socket call returns `result<_, error-code>`: tag 0 is ok.
    ;; Canonical ABI discriminants are u8, so read them with i32.load8_u.
    (func $ok (param $ret i32) (result i32)
      (i32.eqz (i32.load8_u (local.get $ret))))

    ;; `result<_, error-code>` puts the code at offset 1; a
    ;; `result<own<T>, error-code>` aligns to the handle, so it lands at 4.
    (func $die (param $ret i32) (param $offset i32)
      (local $code i32)
      (local.set $code
        (i32.load8_u (i32.add (local.get $ret) (local.get $offset))))
      (i32.store8 (i32.const 0x300)
        (i32.add (i32.const 48) (i32.div_u (local.get $code) (i32.const 10))))
      (i32.store8 (i32.const 0x301)
        (i32.add (i32.const 48) (i32.rem_u (local.get $code) (i32.const 10))))
      (i32.store8 (i32.const 0x302) (i32.const 10))
      (call $eprint (global.get $failed.ptr) (global.get $failed.len))
      (call $eprint (i32.const 0x300) (i32.const 3))
      (call $exit (i32.const 1))
      (unreachable))

    ;; --- serving a file ---------------------------------------------------
    ;; Build the filesystem path at 0x200: "/" becomes "index.html", a
    ;; leading "/" is stripped (WASI resolves only inside a preopen), and a
    ;; trailing "/" gains "index.html".
    (func $fs_path (param $p i32) (param $pl i32) (result i32)
      (local $n i32)
      (if (i32.eq (local.get $pl) (i32.const 1))
        (then
          (memory.copy (i32.const 0x200) (global.get $p-index.ptr)
                       (global.get $p-index.len))
          (return (global.get $p-index.len))))
      (local.set $n (i32.sub (local.get $pl) (i32.const 1)))
      (if (i32.gt_u (local.get $n) (i32.const 400))
        (then (return (i32.const -1))))
      (memory.copy (i32.const 0x200) (i32.add (local.get $p) (i32.const 1))
                   (local.get $n))
      (if (i32.eq (i32.load8_u (i32.add (i32.const 0x200)
                                        (i32.sub (local.get $n) (i32.const 1))))
                  (i32.const 47))
        (then
          (memory.copy (i32.add (i32.const 0x200) (local.get $n))
                       (global.get $p-index.ptr) (global.get $p-index.len))
          (local.set $n (i32.add (local.get $n) (global.get $p-index.len)))))
      (local.get $n))

    ;; Read the whole file at 0x200 into $FILE. -1 when it cannot be opened,
    ;; -2 when it does not fit. The descriptor and the stream are dropped on
    ;; every path out: two handles per request would otherwise accumulate for
    ;; as long as the server runs.
    (func $read_file (param $n i32) (result i32)
      (local $desc i32) (local $stream i32) (local $total i32) (local $got i32)
      (call $open_at (global.get $dir) (i32.const 0)
                     (i32.const 0x200) (local.get $n)
                     (i32.const 0) (i32.const 1) (i32.const 0x180))
      (if (i32.eqz (call $ok (i32.const 0x180)))
        (then (return (i32.const -1))))
      (local.set $desc (i32.load (i32.const 0x184)))
      (call $read_via_stream (local.get $desc) (i64.const 0) (i32.const 0x1A0))
      (if (i32.eqz (call $ok (i32.const 0x1A0)))
        (then (call $drop_desc (local.get $desc)) (return (i32.const -1))))
      (local.set $stream (i32.load (i32.const 0x1A4)))
      (block $done
        (loop $more
          (call $read (local.get $stream) (i64.const 65536) (i32.const 0x120))
          (if (i32.load8_u (i32.const 0x120))
            (then
              ;; `stream-error` case 1 is `closed`, which is end of file.
              (br_if $done
                (i32.eq (i32.load8_u (i32.const 0x124)) (i32.const 1)))
              (local.set $total (i32.const -1))
              (br $done)))
          (local.set $got (i32.load (i32.const 0x128)))
          (br_if $done (i32.eqz (local.get $got)))
          (if (i32.gt_u (i32.add (local.get $total) (local.get $got))
                        (global.get $FILE_CAP))
            (then (local.set $total (i32.const -2)) (br $done)))
          (memory.copy (i32.add (global.get $FILE) (local.get $total))
                       (i32.load (i32.const 0x124)) (local.get $got))
          (local.set $total (i32.add (local.get $total) (local.get $got)))
          (br $more)))
      (call $drop_in (local.get $stream))
      (call $drop_desc (local.get $desc))
      (local.get $total))

    ;; --- the routes -------------------------------------------------------
    ;; Each fills the response buffer. 1 means "and then stop serving".
    (func $route (param $buf i32) (param $len i32) (result i32)
      (local $method i32) (local $p i32) (local $pl i32)
      (local $n i32) (local $plen i32)
      (local $ct i32) (local $ctl i32) (local $body i32)
      (if (call $parse_request (local.get $buf) (local.get $len)
                (i32.const 0x310) (i32.const 0x314) (i32.const 0x318))
        (then (call $r_error (i32.const 400)) (return (i32.const 0))))
      (local.set $method (i32.load (i32.const 0x310)))
      (local.set $p (i32.load (i32.const 0x314)))
      (local.set $pl (i32.load (i32.const 0x318)))

      ;; POST /sha256 -- the body, hex-digested by the provider.
      (if (i32.and
            (i32.eq (local.get $method) (i32.const 1))
            (i32.and (i32.eq (local.get $pl) (global.get $p-sha256.len))
                     (call $eq (local.get $p) (global.get $p-sha256.ptr)
                           (global.get $p-sha256.len))))
        (then
          (local.set $body (call $find_body (local.get $buf) (local.get $len)))
          (if (i32.eqz (local.get $body))
            (then (call $r_error (i32.const 400)) (return (i32.const 0))))
          (call $hash_hex (local.get $body)
            (i32.sub (i32.add (local.get $buf) (local.get $len))
                     (local.get $body))
            (i32.const 0x1C0))
          (call $r_head (i32.const 200)
                        (global.get $m-text.ptr) (global.get $m-text.len)
                        (i32.load (i32.const 0x1C4)))
          (call $r_put (i32.load (i32.const 0x1C0))
                       (i32.load (i32.const 0x1C4)))
          (return (i32.const 0))))
      (if (i32.ne (local.get $method) (i32.const 0))
        (then (call $r_error (i32.const 405)) (return (i32.const 0))))

      ;; GET /quit -- the only thing that ends the run.
      (if (i32.and (i32.eq (local.get $pl) (global.get $p-quit.len))
                   (call $eq (local.get $p) (global.get $p-quit.ptr)
                         (global.get $p-quit.len)))
        (then
          (call $r_head (i32.const 200)
                        (global.get $m-text.ptr) (global.get $m-text.len)
                        (global.get $bye.len))
          (call $r_put (global.get $bye.ptr) (global.get $bye.len))
          (return (i32.const 1))))

      ;; GET /hello -- answered from memory, so a benchmark of this server
      ;; measures the transport rather than a path_open.
      (if (i32.and (i32.eq (local.get $pl) (global.get $p-hello.len))
                   (call $eq (local.get $p) (global.get $p-hello.ptr)
                         (global.get $p-hello.len)))
        (then
          (call $r_head (i32.const 200)
                        (global.get $m-text.ptr) (global.get $m-text.len)
                        (global.get $hello-body.len))
          (call $r_put (global.get $hello-body.ptr)
                       (global.get $hello-body.len))
          (return (i32.const 0))))

      (if (call $has_dotdot (local.get $p) (local.get $pl))
        (then (call $r_error (i32.const 403)) (return (i32.const 0))))
      (local.set $plen (call $fs_path (local.get $p) (local.get $pl)))
      (if (i32.lt_s (local.get $plen) (i32.const 0))
        (then (call $r_error (i32.const 400)) (return (i32.const 0))))
      (local.set $n (call $read_file (local.get $plen)))
      (if (i32.eq (local.get $n) (i32.const -1))
        (then (call $r_error (i32.const 404)) (return (i32.const 0))))
      (if (i32.lt_s (local.get $n) (i32.const 0))
        (then (call $r_error (i32.const 500)) (return (i32.const 0))))
      (call $mime_for (i32.const 0x200) (local.get $plen))
      (local.set $ctl) (local.set $ct)
      (call $r_head (i32.const 200) (local.get $ct) (local.get $ctl)
                    (local.get $n))
      (call $r_put (global.get $FILE) (local.get $n))
      (i32.const 0))

    (func (export "run") (result i32)
      (local $net i32) (local $sock i32) (local $poll i32)
      (local $conn i32) (local $in i32) (local $out i32)
      (local $len i32) (local $mark i32) (local $stop i32)

      ;; The granted directory, resolved once. `get-directories` allocates and
      ;; hands out a descriptor per call, so doing it per request would leak
      ;; one for every connection served.
      (call $get_dirs (i32.const 0x160))
      (if (i32.eqz (i32.load (i32.const 0x164)))
        (then (call $exit (i32.const 2)) (unreachable)))
      (global.set $dir (i32.load (i32.load (i32.const 0x160))))

      (local.set $net (call $network))
      (call $create (i32.const 0) (i32.const 0x1E0))
      (if (i32.eqz (call $ok (i32.const 0x1E0)))
        (then (call $die (i32.const 0x1E0) (i32.const 4))))
      (local.set $sock (i32.load (i32.const 0x1E4)))

      ;; Case 0 is ipv4: port, then the four address bytes. The six slots
      ;; after belong to the ipv6 case and go unread here.
      (call $start_bind
        (local.get $sock) (local.get $net)
        (i32.const 0) (i32.const 8124)
        (i32.const 127) (i32.const 0) (i32.const 0) (i32.const 1)
        (i32.const 0) (i32.const 0) (i32.const 0)
        (i32.const 0) (i32.const 0) (i32.const 0)
        (i32.const 0x1E0))
      (if (i32.eqz (call $ok (i32.const 0x1E0)))
        (then (call $die (i32.const 0x1E0) (i32.const 1))))
      (call $finish_bind (local.get $sock) (i32.const 0x1E0))
      (if (i32.eqz (call $ok (i32.const 0x1E0)))
        (then (call $die (i32.const 0x1E0) (i32.const 1))))
      (call $start_listen (local.get $sock) (i32.const 0x1E0))
      (if (i32.eqz (call $ok (i32.const 0x1E0)))
        (then (call $die (i32.const 0x1E0) (i32.const 1))))
      (call $finish_listen (local.get $sock) (i32.const 0x1E0))
      (if (i32.eqz (call $ok (i32.const 0x1E0)))
        (then (call $die (i32.const 0x1E0) (i32.const 1))))

      (call $print (global.get $ready.ptr) (global.get $ready.len))
      (local.set $poll (call $subscribe (local.get $sock)))

      (block $shutdown
        (loop $serve
          ;; Everything the host allocates for this connection -- the request
          ;; bytes, each file chunk, the digest string -- lives above this
          ;; mark and is released at the bottom of the loop.
          (local.set $mark (call $heap_mark))
          (call $block (local.get $poll))
          (call $accept (local.get $sock) (i32.const 0x100))
          (if (i32.eqz (call $ok (i32.const 0x100)))
            (then (call $die (i32.const 0x100) (i32.const 4))))
          (local.set $conn (i32.load (i32.const 0x104)))
          (local.set $in (i32.load (i32.const 0x108)))
          (local.set $out (i32.load (i32.const 0x10c)))

          ;; One read is assumed to carry the whole request. True of every
          ;; client here; a general server would loop to the end of headers.
          (call $read (local.get $in) (i64.const 8192) (i32.const 0x120))
          (local.set $len
            (select (i32.load (i32.const 0x128)) (i32.const 0)
                    (i32.eqz (i32.load8_u (i32.const 0x120)))))
          (if (i32.eqz (local.get $len))
            (then (call $r_error (i32.const 400)) (local.set $stop (i32.const 0)))
            (else
              (local.set $stop
                (call $route (i32.load (i32.const 0x124)) (local.get $len)))))
          (if (global.get $rfull)
            (then (call $r_error (i32.const 500))))

          (call $write (local.get $out) (global.get $RESP) (global.get $rlen)
            (i32.const 0x140))

          ;; Three handles came in with this connection and three go back.
          ;; The streams first: the host refuses to drop a socket whose
          ;; children are still alive.
          (call $drop_out (local.get $out))
          (call $drop_in (local.get $in))
          (call $drop_socket (local.get $conn))

          (br_if $shutdown (local.get $stop))
          (call $heap_reset (local.get $mark))
          (br $serve)))

      (call $print (global.get $bye.ptr) (global.get $bye.len))
      ;; The pollable is a child of the listening socket, and the host
      ;; refuses to drop a parent while a child is alive.
      (call $drop_pollable (local.get $poll))
      (call $drop_socket (local.get $sock))
      (i32.const 0))
  )
  (core instance $app (instantiate $main
    (with "env" (instance $mem))
    (with "wasi" (instance $wasi))
    (with "net" (instance $net))
    (with "fs" (instance $fs))
    (with "provider" (instance $prov))))

  ;; --- exported wasi:cli/run -------------------------------------------
  (func $run (result (result)) (canon lift (core func $app "run")))
  (instance $run-instance (export "run" (func $run)))
  (export "wasi:cli/run@0.2.12" (instance $run-instance))
)
