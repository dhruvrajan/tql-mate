# tqlmate

dbmate-style migration CLI for TypeDB 3.x. Rust binary `tqlmate`; Scala port under [`scala/`](scala/) builds `tqlmate-scala`.

## Install

```bash
cargo install --path .
# or
cargo build --release   # target/release/tqlmate
```

Requires a TypeDB 3.x server. Default credentials match TypeDB CE (`admin` / `password`).

## URL

```
typedb://user:pass@host:port/database
typedb://admin:password@localhost:1729/typedb          # default
typedb://admin:password@localhost:1729/typedb?tls=true
```

Set via `--url` / `-u`, or `TYPEDB_URL` (alias `DATABASE_URL`). Optional `.env` via `--env-file`.

## Commands

| Command | What it does |
|---------|----------------|
| `new <name>` | Create timestamped migration under `db/migrations` |
| `up` | Create database + run pending migrations |
| `create` / `drop` | Create or delete the database |
| `migrate` | Apply pending migrations |
| `rollback` (`down`) | Roll back the latest applied migration |
| `status` | List applied / pending (`--exit-code`, `--quiet`) |
| `dump` | Write live schema to `db/schema.tql` |
| `load` | Load `db/schema.tql` into the database |
| `wait` | Block until TypeDB accepts connections |

Useful flags: `-d/--migrations-dir`, `--schema-file`, `--wait <secs>`, `--strict`, `-v/--verbose`.

## Example

```bash
export TYPEDB_URL='typedb://admin:password@localhost:1729/app'

tqlmate new create_person
# edit db/migrations/*_create_person.tql

tqlmate up
tqlmate status
tqlmate dump
```

Migration file shape:

```typeql
-- migrate:up
define
  entity person, owns name;
  attribute name, value string;

-- migrate:down
undefine
  owns name from person;
  person;
  name;
```

Each up/down runs in one **SCHEMA** transaction together with the ledger write (`_tqlmate_`-prefixed types). If the migration query fails, the transaction is closed and the version is **not** recorded.

`--strict` refuses pending migrations whose version is lower than an already-applied version.

## Notes

- `dump` uses `Database::schema()` (TypeQL `define` text) and prepends applied versions as comments.
- `load` strips those header comments and runs the remainder as one schema query. Prefer `migrate` for incremental changes; `load` is for bootstrapping from a dump.
- **Unit** (under `tests/`, next to the Docker suite): `url.rs`, `migration.rs`, `ledger.rs`, `cli.rs`. Offline:
  `cargo test --no-default-features --test url --test migration --test ledger --test cli`
  These must not open TypeDB or Docker.
- **Integration** (`tests/typedb_docker.rs` only, feature `typedb-docker`, on by default): TypeDB via [testcontainers](https://testcontainers.com/) (`typedb/typedb:3.12.3`). Requires Docker; fails loudly if unavailable (no silent skip).
- CI (`.github/workflows/ci.yml`, Depot runners): jobs `unit` → `integration`, plus parallel `lint` (rustfmt/clippy).
