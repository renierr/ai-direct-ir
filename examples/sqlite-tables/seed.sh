#!/usr/bin/env bash
# Seed the example database: two tables with a few rows.
# Usage: ./seed.sh [path/to/app.db]  (default: data/app.db beside this script)
set -euo pipefail
cd "$(dirname "$0")"

DB=${1:-data/app.db}
command -v sqlite3 >/dev/null || { echo "sqlite3 not on PATH" >&2; exit 2; }

mkdir -p "$(dirname "$DB")"
rm -f "$DB"

sqlite3 "$DB" <<'SQL'
CREATE TABLE users(id INTEGER PRIMARY KEY, name TEXT NOT NULL);
CREATE TABLE orders(id INTEGER PRIMARY KEY, user_id INTEGER NOT NULL, total REAL NOT NULL);
INSERT INTO users(name) VALUES ('ada'), ('grace'), ('linus');
INSERT INTO orders(user_id, total) VALUES (1, 42.50), (1, 7.25), (2, 100.00);
SQL

printf 'seeded %s:\n' "$DB"
sqlite3 "$DB" "SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name;"
