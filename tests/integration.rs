use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tqlmate::{Opts, Runner, TypeDbUrl};

fn typedb_url() -> TypeDbUrl {
    let raw = std::env::var("TYPEDB_URL")
        .or_else(|_| std::env::var("TQLMATE_TEST_URL"))
        .unwrap_or_else(|_| "typedb://admin:password@127.0.0.1:1729/tqlmate_test".into());
    TypeDbUrl::parse(&raw).expect("TYPEDB_URL")
}

async fn server_up(url: &TypeDbUrl) -> bool {
    let mut runner = Runner::new(Opts {
        url: url.clone(),
        migrations_dir: PathBuf::from("."),
        schema_file: PathBuf::from("schema.tql"),
        strict: false,
        verbose: false,
        wait_timeout: None,
    });
    runner.wait(Duration::from_secs(2)).await.is_ok()
}

fn opts(url: TypeDbUrl, migrations: PathBuf, schema: PathBuf) -> Opts {
    Opts {
        url,
        migrations_dir: migrations,
        schema_file: schema,
        strict: false,
        verbose: true,
        wait_timeout: None,
    }
}

fn require_typedb() -> bool {
    matches!(
        std::env::var("TQLMATE_REQUIRE_TYPEDB").as_deref(),
        Ok("1") | Ok("true") | Ok("TRUE")
    ) || std::env::var("CI").is_ok()
}

fn unique_db(prefix: &str) -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_millis();
    let tid = std::thread::current().id();
    format!("{prefix}_{millis}_{tid:?}")
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect()
}

async fn skip_or_panic_if_down(url: &TypeDbUrl) -> bool {
    if server_up(url).await {
        return false;
    }
    if require_typedb() {
        panic!(
            "TypeDB required but not reachable at {} (set TYPEDB_URL)",
            url.address()
        );
    }
    eprintln!("skip: TypeDB not reachable at {}", url.address());
    true
}

fn write_migration(dir: &std::path::Path, filename: &str, body: &str) {
    std::fs::write(dir.join(filename), body).expect("write migration");
}

#[tokio::test]
async fn migrate_rollback_dump_and_failed_up_not_recorded() {
    let base = typedb_url();
    if skip_or_panic_if_down(&base).await {
        return;
    }

    let url = base.with_database(unique_db("tqlmate_mig"));
    let tmp = tempfile::tempdir().unwrap();
    let migrations = tmp.path().join("migrations");
    let schema = tmp.path().join("schema.tql");
    std::fs::create_dir_all(&migrations).unwrap();

    write_migration(
        &migrations,
        "20240101000000_person.tql",
        "-- migrate:up\n\
         define\n\
           entity person, owns name;\n\
           attribute name, value string;\n\
         \n\
         -- migrate:down\n\
         undefine\n\
           owns name from person;\n\
           person;\n\
           name;\n",
    );
    write_migration(
        &migrations,
        "20240102000000_age.tql",
        "-- migrate:up\n\
         define\n\
           entity person, owns age;\n\
           attribute age, value integer;\n\
         \n\
         -- migrate:down\n\
         undefine\n\
           owns age from person;\n\
           age;\n",
    );

    let mut runner = Runner::new(opts(url, migrations.clone(), schema.clone()));
    let _ = runner.drop().await;
    runner.create().await.expect("create");
    runner.migrate().await.expect("migrate");
    assert!(
        !runner.status(true).await.expect("status"),
        "all migrations should be applied"
    );

    runner.rollback().await.expect("rollback");
    assert!(
        runner.status(true).await.expect("status after rollback"),
        "one migration should be pending after rollback"
    );

    runner.dump().await.expect("dump");
    let dump = std::fs::read_to_string(&schema).expect("read dump");
    assert!(
        dump.contains("-- Schema dumped by tqlmate"),
        "dump should include header"
    );
    assert!(dump.contains("define"), "dump should include schema body");

    runner.migrate().await.expect("re-migrate");

    write_migration(
        &migrations,
        "20240103000000_bad.tql",
        "-- migrate:up\ndefine\n  this is not valid typeql !!!\n\n-- migrate:down\n\n",
    );
    assert!(
        runner.migrate().await.is_err(),
        "invalid TypeQL must fail the SCHEMA transaction"
    );
    assert!(
        runner.status(true).await.expect("status after fail"),
        "failed migration must not be recorded in the ledger"
    );

    runner.drop().await.expect("drop");
}

#[tokio::test]
async fn up_creates_database_and_applies() {
    let base = typedb_url();
    if skip_or_panic_if_down(&base).await {
        return;
    }

    let url = base.with_database(unique_db("tqlmate_up"));
    let tmp = tempfile::tempdir().unwrap();
    let migrations = tmp.path().join("migrations");
    std::fs::create_dir_all(&migrations).unwrap();
    write_migration(
        &migrations,
        "20240101000000_thing.tql",
        "-- migrate:up\ndefine\n  entity thing;\n\n-- migrate:down\nundefine\n  thing;\n",
    );

    let mut runner = Runner::new(opts(url, migrations, tmp.path().join("schema.tql")));
    let _ = runner.drop().await;
    runner.up().await.expect("up");
    assert!(!runner.status(true).await.expect("status"));
    runner.drop().await.expect("drop");
}

#[tokio::test]
async fn empty_up_fails_without_recording() {
    let base = typedb_url();
    if skip_or_panic_if_down(&base).await {
        return;
    }

    let url = base.with_database(unique_db("tqlmate_empty"));
    let tmp = tempfile::tempdir().unwrap();
    let migrations = tmp.path().join("migrations");
    std::fs::create_dir_all(&migrations).unwrap();
    write_migration(
        &migrations,
        "20240101000000_empty.tql",
        "-- migrate:up\n\n\n-- migrate:down\n\n",
    );

    let mut runner = Runner::new(opts(url, migrations, tmp.path().join("schema.tql")));
    let _ = runner.drop().await;
    runner.create().await.expect("create");
    let err = runner.migrate().await.expect_err("empty up should fail");
    assert!(
        err.to_string().contains("empty migrate:up"),
        "unexpected error: {err}"
    );
    assert!(
        runner.status(true).await.expect("status"),
        "empty up must not be recorded"
    );
    runner.drop().await.expect("drop");
}

#[tokio::test]
async fn load_roundtrips_dump() {
    let base = typedb_url();
    if skip_or_panic_if_down(&base).await {
        return;
    }

    let src_url = base.with_database(unique_db("tqlmate_src"));
    let dst_url = base.with_database(unique_db("tqlmate_dst"));
    let tmp = tempfile::tempdir().unwrap();
    let migrations = tmp.path().join("migrations");
    let schema = tmp.path().join("schema.tql");
    std::fs::create_dir_all(&migrations).unwrap();
    write_migration(
        &migrations,
        "20240101000000_widget.tql",
        "-- migrate:up\ndefine\n  entity widget;\n\n-- migrate:down\nundefine\n  widget;\n",
    );

    let mut src = Runner::new(opts(src_url, migrations.clone(), schema.clone()));
    let _ = src.drop().await;
    src.up().await.expect("up src");
    src.dump().await.expect("dump");

    let mut dst = Runner::new(opts(dst_url, migrations, schema));
    let _ = dst.drop().await;
    dst.load().await.expect("load");

    src.drop().await.expect("drop src");
    dst.drop().await.expect("drop dst");
}
