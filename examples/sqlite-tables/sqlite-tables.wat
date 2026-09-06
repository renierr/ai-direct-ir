;; List every table in data/app.db, one name per line.
;; Runs `SELECT name FROM sqlite_master WHERE type='table' ORDER BY name`
;; through the ai-direct:sqlite provider and prints the text column.
;;
;; Memory map (17 pages): 0x200 write result, 0x300 open result,
;;   0x320 exec result, 0x340 close result,
;;   0x1000..0x2000 named segments, 0x40000+ canonical ABI bump allocation
;;
;; result-set at R: disc R, columns ptr/len at R+4/R+8, rows ptr/len at
;; R+12/R+16. A row is values ptr/len (8 bytes); a text value is disc 2
;; at +0 with ptr/len at +8/+12. All discriminants in memory are u8.

(component
  ;; @wasi stdout pages=17 heap=0x40000
  ;; @data 0x1000..0x2000

  ;; Provider nominal types need the eq-export pattern (see docs/AUTHORING.md
  ;; "Importing provider types"): an imported instance may not define them
  ;; inline, so each declares locally and exports the equality.
  (import "ai-direct:sqlite/store@0.1.0" (instance $s
    (type $value-l (variant
      (case "int-val" s64) (case "real-val" f64)
      (case "text-val" string) (case "blob-val" (list u8)) (case "null-val")))
    (export "value" (type $value-x (eq $value-l)))
    (type $row-l (record (field "values" (list $value-x))))
    (export "row" (type $row-x (eq $row-l)))
    (type $result-set-l (record
      (field "columns" (list string)) (field "rows" (list $row-x))))
    (export "result-set" (type $result-set-x (eq $result-set-l)))
    (export "open" (func (param "path" string) (result (result u32 (error string)))))
    (export "exec" (func (param "handle" u32) (param "sql" string)
      (param "params" (list $value-x)) (result (result $result-set-x (error string)))))
    (export "close" (func (param "handle" u32) (result (result (error string)))))))
  (alias export $s "open" (func $open))
  (alias export $s "exec" (func $exec))
  (alias export $s "close" (func $close))
  (core func $open-l (canon lower (func $open) (memory $memory) (realloc $realloc)))
  (core func $exec-l (canon lower (func $exec) (memory $memory) (realloc $realloc)))
  (core func $close-l (canon lower (func $close) (memory $memory) (realloc $realloc)))
  (core instance $prov
    (export "open" (func $open-l))
    (export "exec" (func $exec-l))
    (export "close" (func $close-l)))

  (core module $main
    (import "env" "memory" (memory 17))
    (import "wasi" "get-stdout" (func $get_stdout (result i32)))
    (import "wasi" "write" (func $write (param i32 i32 i32 i32)))
    (import "provider" "open" (func $open (param i32 i32 i32)))
    (import "provider" "exec" (func $exec (param i32 i32 i32 i32 i32 i32)))
    (import "provider" "close" (func $close (param i32 i32)))

    (global $handle (mut i32) (i32.const 0))
    (data $nl "\n")
    (data $fail "FAIL\n")

    (func $print (param $ptr i32) (param $len i32)
      (call $write (call $get_stdout) (local.get $ptr) (local.get $len)
        (i32.const 0x200))
      (drop (i32.load (i32.const 0x200))))

    (func $fail (result i32)
      (call $print (global.get $fail.ptr) (global.get $fail.len))
      (i32.const 1))

    (func $check (param $ret i32) (result i32)
      (i32.load8_u (local.get $ret)))

    (func (export "run") (result i32)
      (local $rows i32) (local $nrows i32) (local $i i32)
      (local $row i32) (local $val i32)
      ;; open("data/app.db")
      (call $open (global.get $path.ptr) (global.get $path.len) (i32.const 0x300))
      (if (call $check (i32.const 0x300)) (then (return (call $fail))))
      (global.set $handle (i32.load (i32.const 0x304)))
      ;; SELECT the table names; no bound params.
      (call $exec (global.get $handle)
        (global.get $sql.ptr) (global.get $sql.len)
        (i32.const 0) (i32.const 0) (i32.const 0x320))
      (if (call $check (i32.const 0x320)) (then (return (call $fail))))
      (local.set $rows (i32.load (i32.const 0x32C)))
      (local.set $nrows (i32.load (i32.const 0x330)))
      (local.set $i (i32.const 0))
      (block $done
        (loop $more
          (br_if $done (i32.ge_u (local.get $i) (local.get $nrows)))
          ;; first (only) column of row $i must be text; print it.
          (local.set $row (i32.add (local.get $rows)
            (i32.mul (local.get $i) (i32.const 8))))
          (local.set $val (i32.load (local.get $row)))
          (if (i32.ne (i32.load8_u (local.get $val)) (i32.const 2))
            (then (return (call $fail))))
          (call $print
            (i32.load (i32.add (local.get $val) (i32.const 8)))
            (i32.load (i32.add (local.get $val) (i32.const 12))))
          (call $print (global.get $nl.ptr) (global.get $nl.len))
          (local.set $i (i32.add (local.get $i) (i32.const 1)))
          (br $more)))
      (call $close (global.get $handle) (i32.const 0x340))
      (if (call $check (i32.const 0x340)) (then (return (call $fail))))
      (i32.const 0))

    (data $path "data/app.db")
    (data $sql "SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name"))

  (core instance $app (instantiate $main
    (with "env" (instance $mem))
    (with "wasi" (instance $wasi))
    (with "provider" (instance $prov))))
  (func $run (result (result)) (canon lift (core func $app "run")))
  (instance $run-i (export "run" (func $run)))
  (export "wasi:cli/run@0.2.12" (instance $run-i))
)
