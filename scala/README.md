# tqlmate (Scala)

Scala 3 port of the Rust `tqlmate` CLI. Same commands, URL, migration format, and SCHEMA+ledger semantics. Ship as **`tqlmate-scala`** so it does not clash with the Rust binary.

## Run (JVM)

```bash
cd scala
scala-cli run . -- status
scala-cli run . -- -u 'typedb://admin:password@localhost:1729/app' up
```

Global flags go **before** the subcommand: `tqlmate-scala -v migrate`, not `migrate -v`.

## Test

```bash
scala-cli test .
TYPEDB_URL='typedb://admin:password@127.0.0.1:1729/tqlmate_test' scala-cli test .
```

Integration tests skip if TypeDB is unreachable; set `TQLMATE_REQUIRE_TYPEDB=1` (or `CI`) to fail instead.

## Package

### Assembly JAR (always works)

```bash
scala-cli --power package --assembly . --force -o tqlmate-scala.jar
./tqlmate-scala.jar --help
```

### GraalVM native-image

TypeDB’s Java driver is **JNI/SWIG**, so Scala Native cannot use it. Use GraalVM native-image. JNI/reflect metadata under `resources/META-INF/native-image/` was generated with the native-image agent against a live server.

```bash
scala-cli --power package --native-image . \
  --force -o tqlmate-scala \
  --graalvm-args --no-fallback \
  --graalvm-args --enable-url-protocols=http,https \
  --graalvm-args -H:+ReportExceptionStackTraces
./tqlmate-scala --help
```

If you upgrade `typedb-driver`, regenerate agent config:

```bash
scala-cli --power package --assembly . -o tqlmate-scala.jar
java -agentlib:native-image-agent=config-output-dir=resources/META-INF/native-image/tqlmate-scala \
  -jar tqlmate-scala.jar -u "$TYPEDB_URL" create
# merge further commands with config-merge-dir=...
```

## URL

```
typedb://admin:password@localhost:1729/typedb
typedb://admin:password@localhost:1729/typedb?tls=true
```

`--url` / `-u`, or `TYPEDB_URL` (`DATABASE_URL` alias).

## Layout

Mirrors the Rust modules: `Main` (decline CLI), `Url`, `Migration`, `Ledger`, `Runner`, `Error`.
