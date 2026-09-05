;; tcp-hello.wat -- a WASI 0.2 component that owns its own listening socket.
;;
;; `;; @wasi ... sockets` generates the whole boundary: the four
;; `wasi:sockets` interfaces this program touches, `wasi:io/poll` for the
;; blocking wait, the shared memory and every canonical ABI lowering. The
;; `(import "net" ...)` lines below decide its extent -- twelve names out of
;; the WIT's thirty-nine, and no UDP.
;;
;; The host still has to grant the network: `wasi:sockets` is linked for every
;; component and every call answers `access-denied` until a manifest says
;; `network = true` (or the command line says `--net`).
;;
;; It accepts connections in a loop and stops on `GET /quit`. Each connection
;; hands back three handles -- the accepted socket and its two streams -- and
;; each is released with `resource.drop` before the next accept. That is not
;; housekeeping: dropping the socket is what closes the connection, so a
;; client only sees end-of-stream because the drop happened. A server that
;; skipped it would leave every client hanging and leak a handle per request.
;;
;; Handles are not the only thing a loop has to give back. `blocking-read`
;; allocates its `list<u8>` through the boundary's bump heap, which frees
;; nothing on its own; before `heap-mark`/`heap-reset` this program died after
;; 420 requests with `realloc return: beyond end of memory`. The mark taken at
;; the top of the loop and restored at the bottom bounds the whole run to one
;; connection's allocation.
;;
;; Memory map (1 page):
;;   0x100..0x200 text, packed by `;; @data`
;;   0x200 create-tcp-socket result   (tag @0, socket/error @4)
;;   0x210 start-bind result          (tag @0, error @1)
;;   0x220 finish-bind result
;;   0x230 start-listen result
;;   0x240 finish-listen result
;;   0x250 accept result              (tag @0, socket @4, input @8, output @12)
;;   0x280 blocking-read result       (tag @0, ptr @4, len @8)
;;   0x290 blocking-write result
;;   0x1000 three bytes of scratch, for rendering an error code
;;   0x8000+ canonical ABI bump allocation

(component
  ;; @wasi stdout stderr exit-with-code sockets

  ;; --- application logic, ordinary Core WAT -----------------------------
  (core module $main
    (import "env" "memory" (memory 1))
    ;; The canonical ABI allocates every host-produced value -- here, each
    ;; request's `list<u8>` -- out of a bump heap that never frees. A mark
    ;; taken at the top of the loop and restored at the bottom releases the
    ;; whole iteration at once.
    (import "env" "heap-mark" (func $heap_mark (result i32)))
    (import "env" "heap-reset" (func $heap_reset (param i32)))
    (import "wasi" "get-stdout" (func $get_stdout (result i32)))
    (import "wasi" "get-stderr" (func $get_stderr (result i32)))
    (import "wasi" "read" (func $read (param i32 i64 i32)))
    (import "wasi" "write" (func $write (param i32 i32 i32 i32)))
    (import "wasi" "exit-with-code" (func $exit (param i32)))

    ;; A resource the boundary declares can be released. The stream resources
    ;; belong to `$wasi`, because stdio hands them out too.
    (import "wasi" "input-stream.drop" (func $drop_in (param i32)))
    (import "wasi" "output-stream.drop" (func $drop_out (param i32)))

    ;; Nine of the thirty-nine functions `wasi:sockets` declares, plus the
    ;; drops for the two resources this program holds. A method is named by
    ;; its resource, because five different resources have a `subscribe`.
    (import "net" "instance-network" (func $network (result i32)))
    (import "net" "create-tcp-socket" (func $create (param i32 i32)))
    ;; `ip-socket-address` is a variant, flattened into the parameter list:
    ;; one slot for the case, then the widest case's payload. Hence fifteen.
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

    ;; @data 0x100..0x200
    (data $body
      "HTTP/1.1 200 OK\r\n"
      "Content-Type: text/plain; charset=utf-8\r\n"
      "Content-Length: 12\r\n"
      "Connection: close\r\n"
      "\r\n"
      "hello, air!\n")
    (data $ready "tcp-hello: listening on 127.0.0.1:8125\n")
    (data $bye "tcp-hello: /quit\n")
    (data $quit "GET /quit")
    (data $failed "tcp-hello: wasi:sockets error-code ")

    (func $print (param $p i32) (param $n i32)
      (call $write (call $get_stdout) (local.get $p) (local.get $n)
        (i32.const 0x290)))

    (func $eprint (param $p i32) (param $n i32)
      (call $write (call $get_stderr) (local.get $p) (local.get $n)
        (i32.const 0x290)))

    ;; Every socket call returns `result<_, error-code>`: tag 0 is ok. Canonical
    ;; ABI discriminants are u8, so read them with i32.load8_u -- an i32.load
    ;; would take three bytes of undefined padding along with the tag.
    (func $ok (param $ret i32) (result i32)
      (i32.eqz (i32.load8_u (local.get $ret))))

    ;; Where the error code sits depends on the ok payload's alignment.
    ;; `result<_, error-code>` is two u8s, so the code is at offset 1;
    ;; `result<own<T>, error-code>` aligns to the four-byte handle, so the code
    ;; is at offset 4. The caller knows which of the two it asked for.
    (func $die (param $ret i32) (param $offset i32)
      (local $code i32)
      (local.set $code
        (i32.load8_u (i32.add (local.get $ret) (local.get $offset))))
      (i32.store8 (i32.const 0x1000)
        (i32.add (i32.const 48) (i32.div_u (local.get $code) (i32.const 10))))
      (i32.store8 (i32.const 0x1001)
        (i32.add (i32.const 48) (i32.rem_u (local.get $code) (i32.const 10))))
      (i32.store8 (i32.const 0x1002) (i32.const 10))
      (call $eprint (global.get $failed.ptr) (global.get $failed.len))
      (call $eprint (i32.const 0x1000) (i32.const 3))
      (call $exit (i32.const 1))
      (unreachable))

    ;; Whether the `$n` bytes at `$p` begin with the `$qn` bytes at `$q`.
    (func $starts_with
      (param $p i32) (param $n i32) (param $q i32) (param $qn i32) (result i32)
      (local $i i32)
      (if (i32.lt_u (local.get $n) (local.get $qn))
        (then (return (i32.const 0))))
      (block $done
        (loop $next
          (br_if $done (i32.ge_u (local.get $i) (local.get $qn)))
          (if (i32.ne
                (i32.load8_u (i32.add (local.get $p) (local.get $i)))
                (i32.load8_u (i32.add (local.get $q) (local.get $i))))
            (then (return (i32.const 0))))
          (local.set $i (i32.add (local.get $i) (i32.const 1)))
          (br $next)))
      (i32.const 1))

    (func (export "run") (result i32)
      (local $net i32)
      (local $sock i32)
      (local $poll i32)
      (local $conn i32)
      (local $in i32)
      (local $out i32)
      (local $len i32)
      (local $mark i32)

      (local.set $net (call $network))

      ;; create-tcp-socket(ipv4) -> result<own<tcp-socket>, error-code>.
      ;; Without a network grant this is where the run stops, with code 1,
      ;; `access-denied`.
      (call $create (i32.const 0) (i32.const 0x200))
      (if (i32.eqz (call $ok (i32.const 0x200)))
        (then (call $die (i32.const 0x200) (i32.const 4))))
      (local.set $sock (i32.load (i32.const 0x204)))

      ;; start-bind(self, network, ip-socket-address) -> result<_, error-code>.
      ;; Case 0 is ipv4: port, then the four address bytes. The six slots after
      ;; belong to the ipv6 case and go unread for an ipv4 address.
      (call $start_bind
        (local.get $sock) (local.get $net)
        (i32.const 0)                                   ;; case ipv4
        (i32.const 8125)                                ;; port
        (i32.const 127) (i32.const 0) (i32.const 0) (i32.const 1)
        (i32.const 0) (i32.const 0) (i32.const 0)
        (i32.const 0) (i32.const 0) (i32.const 0)
        (i32.const 0x210))
      (if (i32.eqz (call $ok (i32.const 0x210)))
        (then (call $die (i32.const 0x210) (i32.const 1))))
      (call $finish_bind (local.get $sock) (i32.const 0x220))
      (if (i32.eqz (call $ok (i32.const 0x220)))
        (then (call $die (i32.const 0x220) (i32.const 1))))

      (call $start_listen (local.get $sock) (i32.const 0x230))
      (if (i32.eqz (call $ok (i32.const 0x230)))
        (then (call $die (i32.const 0x230) (i32.const 1))))
      (call $finish_listen (local.get $sock) (i32.const 0x240))
      (if (i32.eqz (call $ok (i32.const 0x240)))
        (then (call $die (i32.const 0x240) (i32.const 1))))

      (call $print (global.get $ready.ptr) (global.get $ready.len))

      ;; WASI 0.2 has no blocking accept: subscribe to the listening socket and
      ;; block on the pollable until a connection is waiting. One pollable
      ;; serves every accept, so it is created once, outside the loop.
      (local.set $poll (call $subscribe (local.get $sock)))

      (block $stop
        (loop $serve
          ;; Everything the host allocates for this connection lives above
          ;; here and dies at the bottom of the loop.
          (local.set $mark (call $heap_mark))
          (call $block (local.get $poll))

          ;; accept -> result<tuple<own<tcp-socket>, own<input-stream>,
          ;;                        own<output-stream>>, error-code>
          (call $accept (local.get $sock) (i32.const 0x250))
          (if (i32.eqz (call $ok (i32.const 0x250)))
            (then (call $die (i32.const 0x250) (i32.const 4))))
          (local.set $conn (i32.load (i32.const 0x254)))
          (local.set $in (i32.load (i32.const 0x258)))
          (local.set $out (i32.load (i32.const 0x25c)))

          ;; An accepted connection is an ordinary `input-stream`, drained with
          ;; the same `blocking-read` stdin uses. One read is enough for a
          ;; request line; a client that hung up first reads as an error, and
          ;; an empty request matches no path.
          (call $read (local.get $in) (i64.const 4096) (i32.const 0x280))
          (local.set $len
            (select (i32.load (i32.const 0x288)) (i32.const 0)
                    (call $ok (i32.const 0x280))))

          ;; A failed write here means the client left; that is its business,
          ;; not a reason to stop serving.
          (call $write (local.get $out)
            (global.get $body.ptr) (global.get $body.len)
            (i32.const 0x290))

          ;; Three handles came in with this connection and three go back out.
          ;; The streams first: they borrow the socket the drop below closes.
          (call $drop_out (local.get $out))
          (call $drop_in (local.get $in))
          (call $drop_socket (local.get $conn))

          ;; The request bytes are still on the heap, so the reset comes
          ;; after the last read of them.
          (br_if $stop
            (call $starts_with
              (i32.load (i32.const 0x284)) (local.get $len)
              (global.get $quit.ptr) (global.get $quit.len)))
          (call $heap_reset (local.get $mark))
          (br $serve)))

      (call $print (global.get $bye.ptr) (global.get $bye.len))
      (call $drop_pollable (local.get $poll))
      (call $drop_socket (local.get $sock))
      (i32.const 0))
  )
  (core instance $app (instantiate $main
    (with "env" (instance $mem))
    (with "wasi" (instance $wasi))
    (with "net" (instance $net))))

  ;; --- exported wasi:cli/run -------------------------------------------
  (func $run (result (result)) (canon lift (core func $app "run")))
  (instance $run-instance (export "run" (func $run)))
  (export "wasi:cli/run@0.2.12" (instance $run-instance))
)
