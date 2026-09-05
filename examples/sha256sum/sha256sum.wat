;; sha256sum.wat -- a real CLI: hash a file, or stdin, and print the digest.
;;
;;   air run examples/sha256sum/host.toml FILE
;;   air run examples/sha256sum/host.toml -        # read stdin
;;   air run examples/sha256sum/host.toml --help
;;
;; The cryptography is not here. `ai-direct:sha256/digest` comes from a vendored
;; provider package built from the RustCrypto sha2 crate; this file is argument
;; handling, file reading, and output.
;;
;; The wasi:filesystem boundary below is generated from WIT, not transcribed:
;; `filesystem` on the `;; @wasi` line derives every type and signature from
;; `air/wit/wasi-0.2.12/filesystem.wit`. Nothing in `air` hardcodes the word
;; "filesystem" beyond wiring that file: the harness links the whole WASI 0.2
;; set, so a new interface needs a declaration here, not a change there.
;;
;; Memory map (17 pages): 0x200 write result, 0x300 digest result,
;;   0x400 stream read result, 0x500 arguments, 0x600 preopens,
;;   0x700 open-at result, 0x800 read-via-stream result,
;;   0x1000..0x2000 `;; @data` strings, 0x10000..0x40000 file buffer,
;;   0x40000+ canonical ABI bump allocation

(component
  ;; @wasi stdin stdout stderr args exit-with-code filesystem pages=17 heap=0x40000
  ;; @data 0x1000..0x2000

  ;; --- the provider, imported like any other interface --------------------
  (import "ai-direct:sha256/digest@0.1.0" (instance $sha
    (export "hash-hex" (func (param "data" (list u8)) (result string)))))
  (alias export $sha "hash-hex" (func $hash-hex))
  (core func $hash-hex-l
    (canon lower (func $hash-hex) (memory $memory) (realloc $realloc)))
  (core instance $prov (export "hash-hex" (func $hash-hex-l)))

  ;; --- application logic, ordinary Core WAT -------------------------------
  (core module $main
    (import "env" "memory" (memory 17))
    (import "wasi" "get-stdin" (func $get_stdin (result i32)))
    (import "wasi" "get-stdout" (func $get_stdout (result i32)))
    (import "wasi" "get-stderr" (func $get_stderr (result i32)))
    (import "wasi" "read" (func $read (param i32 i64 i32)))
    (import "wasi" "write" (func $write (param i32 i32 i32 i32)))
    (import "wasi" "get-arguments" (func $get_args (param i32)))
    (import "wasi" "exit-with-code" (func $exit (param i32)))
    (import "fs" "get-directories" (func $get_dirs (param i32)))
    (import "fs" "open-at" (func $open_at (param i32 i32 i32 i32 i32 i32 i32)))
    (import "fs" "read-via-stream" (func $read_via_stream (param i32 i64 i32)))
    (import "provider" "hash-hex" (func $hash_hex (param i32 i32 i32)))

    (global $BUF i32 (i32.const 0x10000))
    (global $CAP i32 (i32.const 0x30000))

    (data $usage
      "sha256sum -- print the SHA-256 digest of a file\n"
      "\n"
      "usage:\n"
      "  air run host.toml <file>   digest a file under the project root\n"
      "  air run host.toml -        digest standard input\n"
      "  air run host.toml --help   show this message\n"
      "\n"
      "The digest is computed by the vendored ai-direct:sha256 provider,\n"
      "built from the RustCrypto sha2 crate. Paths resolve inside the\n"
      "directory the manifest preopens.\n")
    (data $err-args "sha256sum: expected one file argument; try --help\n")
    (data $err-open "sha256sum: cannot open ")
    (data $err-hint
      "\n"
      "  Only granted directories are readable, and WASI has no global root.\n"
      "  Grant one with:  air run --dir . <manifest> <file>\n"
      "  or set `root` in the manifest (`root = \"/\"` grants everything).\n")
    (data $err-read "sha256sum: read failed\n")
    (data $err-big  "sha256sum: input is larger than the buffer\n")
    (data $err-root "sha256sum: no preopened directory; set `root` in host.toml\n")
    (data $help-long "--help")
    (data $help-short "-h")
    (data $dash "-")
    (data $gap "  ")
    (data $nl "\n")

    (func $print (param $p i32) (param $n i32)
      (call $write (call $get_stdout) (local.get $p) (local.get $n)
        (i32.const 0x200)))

    (func $eprint (param $p i32) (param $n i32)
      (call $write (call $get_stderr) (local.get $p) (local.get $n)
        (i32.const 0x200)))

    (func $die (param $p i32) (param $n i32) (param $code i32)
      (call $eprint (local.get $p) (local.get $n))
      (call $exit (local.get $code))
      (unreachable))

    ;; 1 when the two byte ranges are equal.
    (func $streq (param $a i32) (param $an i32) (param $b i32) (param $bn i32)
      (result i32)
      (local $i i32)
      (if (i32.ne (local.get $an) (local.get $bn))
        (then (return (i32.const 0))))
      (block $done
        (loop $each
          (br_if $done (i32.ge_u (local.get $i) (local.get $an)))
          (if (i32.ne (i32.load8_u (i32.add (local.get $a) (local.get $i)))
                      (i32.load8_u (i32.add (local.get $b) (local.get $i))))
            (then (return (i32.const 0))))
          (local.set $i (i32.add (local.get $i) (i32.const 1)))
          (br $each)))
      (i32.const 1))

    ;; Drain an input-stream into $BUF. Returns the byte count, or -1.
    ;;
    ;; The result of blocking-read is `result<list<u8>, stream-error>`:
    ;; 0x400 is the outer discriminant, 0x404/0x408 the list on ok. On err,
    ;; 0x404 is the stream-error case -- 1 is `closed`, the normal end.
    ;; Discriminants are u8, so they are read with i32.load8_u: an i32.load
    ;; would take three bytes of undefined padding with the tag.
    (func $drain (param $stream i32) (result i32)
      (local $total i32) (local $n i32)
      (block $done
        (loop $more
          (call $read (local.get $stream) (i64.const 65536) (i32.const 0x400))
          (if (i32.load8_u (i32.const 0x400))
            (then
              (br_if $done
                (i32.eq (i32.load8_u (i32.const 0x404)) (i32.const 1)))
              (return (i32.const -1))))
          (local.set $n (i32.load (i32.const 0x408)))
          (br_if $done (i32.eqz (local.get $n)))
          (if (i32.gt_u (i32.add (local.get $total) (local.get $n))
                        (global.get $CAP))
            (then (call $die (global.get $err-big.ptr)
                             (global.get $err-big.len) (i32.const 2))))
          (memory.copy (i32.add (global.get $BUF) (local.get $total))
                       (i32.load (i32.const 0x404)) (local.get $n))
          (local.set $total (i32.add (local.get $total) (local.get $n)))
          (br $more)))
      (local.get $total))

    ;; Open a path under a granted directory and drain it.
    ;;
    ;; WASI has no global filesystem root: `open-at` resolves only inside a
    ;; preopened descriptor, and a leading `/` is not a path to anywhere. So an
    ;; absolute argument is made relative and every granted directory is tried
    ;; in turn -- with `--dir /` that makes an absolute path work as written.
    (func $read_file (param $p i32) (param $n i32) (result i32)
      (local $dirs i32) (local $count i32) (local $i i32) (local $opened i32)
      (call $get_dirs (i32.const 0x600))
      (local.set $dirs (i32.load (i32.const 0x600)))
      (local.set $count (i32.load (i32.const 0x604)))
      (if (i32.eqz (local.get $count))
        (then (call $die (global.get $err-root.ptr)
                         (global.get $err-root.len) (i32.const 2))))

      (if (i32.eq (i32.load8_u (local.get $p)) (i32.const 47))
        (then
          (local.set $p (i32.add (local.get $p) (i32.const 1)))
          (local.set $n (i32.sub (local.get $n) (i32.const 1)))))

      (local.set $opened (i32.const -1))
      (block $found
        (loop $each
          (br_if $found (i32.ge_u (local.get $i) (local.get $count)))
          ;; element i is [descriptor, name-ptr, name-len]
          ;; open-at(self, path-flags=0, path, open-flags=0, flags=read)
          (call $open_at
            (i32.load (i32.add (local.get $dirs)
                               (i32.mul (local.get $i) (i32.const 12))))
            (i32.const 0) (local.get $p) (local.get $n)
            (i32.const 0) (i32.const 1) (i32.const 0x700))
          (if (i32.eqz (i32.load8_u (i32.const 0x700)))
            (then
              (local.set $opened (i32.load (i32.const 0x704)))
              (br $found)))
          (local.set $i (i32.add (local.get $i) (i32.const 1)))
          (br $each)))

      (if (i32.lt_s (local.get $opened) (i32.const 0))
        (then
          (call $eprint (global.get $err-open.ptr) (global.get $err-open.len))
          (call $eprint (local.get $p) (local.get $n))
          (call $die (global.get $err-hint.ptr)
                     (global.get $err-hint.len) (i32.const 2))))

      (call $read_via_stream (local.get $opened) (i64.const 0)
        (i32.const 0x800))
      (if (i32.load8_u (i32.const 0x800))
        (then (call $die (global.get $err-read.ptr)
                         (global.get $err-read.len) (i32.const 2))))
      (call $drain (i32.load (i32.const 0x804))))

    (func (export "run") (result i32)
      (local $argv i32) (local $argc i32) (local $ap i32) (local $an i32)
      (local $n i32)
      (call $get_args (i32.const 0x500))
      (local.set $argv (i32.load (i32.const 0x500)))
      (local.set $argc (i32.load (i32.const 0x504)))

      ;; argv[0] is the program name; the file argument is argv[1].
      (if (i32.ne (local.get $argc) (i32.const 2))
        (then (call $die (global.get $err-args.ptr)
                         (global.get $err-args.len) (i32.const 1))))
      (local.set $ap (i32.load offset=8 (local.get $argv)))
      (local.set $an (i32.load offset=12 (local.get $argv)))

      (if (i32.or
            (call $streq (local.get $ap) (local.get $an)
                  (global.get $help-long.ptr) (global.get $help-long.len))
            (call $streq (local.get $ap) (local.get $an)
                  (global.get $help-short.ptr) (global.get $help-short.len)))
        (then
          (call $print (global.get $usage.ptr) (global.get $usage.len))
          (return (i32.const 0))))

      (if (call $streq (local.get $ap) (local.get $an)
                (global.get $dash.ptr) (global.get $dash.len))
        (then (local.set $n (call $drain (call $get_stdin))))
        (else (local.set $n (call $read_file (local.get $ap) (local.get $an)))))
      (if (i32.lt_s (local.get $n) (i32.const 0))
        (then (call $die (global.get $err-read.ptr)
                         (global.get $err-read.len) (i32.const 2))))

      ;; "<hex>  <name>" -- two spaces, like coreutils.
      (call $hash_hex (global.get $BUF) (local.get $n) (i32.const 0x300))
      (call $print (i32.load (i32.const 0x300)) (i32.load (i32.const 0x304)))
      (call $print (global.get $gap.ptr) (global.get $gap.len))
      (call $print (local.get $ap) (local.get $an))
      (call $print (global.get $nl.ptr) (global.get $nl.len))
      (i32.load (i32.const 0x200))))

  (core instance $app (instantiate $main
    (with "env" (instance $mem))
    (with "wasi" (instance $wasi))
    (with "fs" (instance $fs))
    (with "provider" (instance $prov))))
  (func $run (result (result)) (canon lift (core func $app "run")))
  (instance $run-i (export "run" (func $run)))
  (export "wasi:cli/run@0.2.12" (instance $run-i))
)
