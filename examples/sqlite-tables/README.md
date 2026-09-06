# sqlite-tables

A small application that opens a SQLite database through the
`ai-direct:sqlite` provider and prints every table, one name per line.
It runs `SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY
name` and prints the text column — the read path of the provider's
one-shot `exec` interface.

Install the locked provider once after cloning, seed the demo database,
then run it:

```bash
cd examples/sqlite-tables
air add --from ../../../ai-direct-ir-providers/providers/sqlite ai-direct:sqlite@0.1.0
./seed.sh            # creates data/app.db with users + orders tables
air run
```

Expected output:

```text
orders
users
```

`./seed.sh [path]` takes an optional database path (default
`data/app.db`). The application contains only WAT, `host.toml`, and
`air.lock`; the reviewed provider package is cached under
`~/.cache/air/providers/<artifact-hash>/`. The database file itself is
local state and is not committed.
