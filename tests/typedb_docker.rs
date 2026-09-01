//! TypeDB Docker integration suite (`cargo test --test typedb_docker`).
//!
//! Starts TypeDB 3.x via testcontainers and drives `tqlmate::Runner` against it.
//! Panics if Docker cannot start TypeDB (no silent skip).
//!
//! Pure unit tests live only under `src/**` (`cargo test --lib --bins`) and never
//! import testcontainers or open a TypeDB connection.

#![cfg(feature = "typedb-docker")]

use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use testcontainers::core::{IntoContainerPort, WaitFor};
use testcontainers::runners::AsyncRunner;
use testcontainers::{ContainerAsync, GenericImage, ImageExt};
use tokio::sync::OnceCell;
use tqlmate::{Opts, Runner, TypeDbUrl};

const TYPEDB_IMAGE: &str = "typedb/typedb";
const TYPEDB_TAG: &str = "3.12.3";
const TYPEDB_PORT: u16 = 1729;

struct SharedTypeDb {
    #[allow(dead_code)]
    container: ContainerAsync<GenericImage>,
    host: String,
    port: u16,
}

impl SharedTypeDb {
    fn url_for(&self, database: &str) -> TypeDbUrl {
        TypeDbUrl::parse(&format!(
            "typedb://admin:password@{}:{}/{database}",
            self.host, self.port
        ))
        .expect("typedb url")
    }
}

async fn shared_typedb() -> &'static SharedTypeDb {
    static CELL: OnceCell<SharedTypeDb> = OnceCell::const_new();
    CELL.get_or_init(|| async {
        if std::process::Command::new("docker")
            .arg("info")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| !s.success())
            .unwrap_or(true)
        {
            panic!(
                "typedb_docker tests require Docker, but `docker info` failed. \
                 Install/start Docker, or run unit tests only: `cargo test --lib --bins`."
            );
        }

        let image = GenericImage::new(TYPEDB_IMAGE, TYPEDB_TAG)
            .with_exposed_port(TYPEDB_PORT.tcp())
            .with_wait_for(WaitFor::message_on_stderr("Ready!"))
            .with_startup_timeout(Duration::from_secs(120));

        let container = image.start().await.unwrap_or_else(|e| {
            panic!("failed to start {TYPEDB_IMAGE}:{TYPEDB_TAG} via testcontainers: {e}")
        });

        let host = container
            .get_host()
            .await
            .unwrap_or_else(|e| panic!("typedb container host: {e}"))
            .to_string();
        let port = container
            .get_host_port_ipv4(TYPEDB_PORT)
            .await
            .unwrap_or_else(|e| panic!("typedb mapped port: {e}"));

        let probe = TypeDbUrl::parse(&format!("typedb://admin:password@{host}:{port}/typedb"))
            .expect("probe url");
        let mut waiter = Runner::new(Opts {
            url: probe,
            migrations_dir: PathBuf::from("."),
            schema_file: PathBuf::from("schema.tql"),
            strict: false,
            verbose: false,
            wait_timeout: None,
        });
        waiter
            .wait(Duration::from_secs(120))
            .await
            .unwrap_or_else(|e| panic!("TypeDB container started but driver wait failed: {e}"));

        SharedTypeDb {
            container,
            host,
            port,
        }
    })
    .await
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

fn write_migration(dir: &std::path::Path, filename: &str, body: &str) {
    std::fs::write(dir.join(filename), body).expect("write migration");
}

#[tokio::test]
async fn migrate_status_dump_rollback_failed_up_and_drop() {
    let typedb = shared_typedb().await;
    let url = typedb.url_for(&unique_db("tqlmate_mig"));
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
    runner.migrate().await.expect("migrate two");
    assert!(
        !runner.status(true).await.expect("status"),
        "both migrations should be applied"
    );

    runner.dump().await.expect("dump");
    let dump = std::fs::read_to_string(&schema).expect("read dump");
    assert!(
        dump.contains("-- Schema dumped by tqlmate"),
        "dump header missing: {dump}"
    );
    assert!(
        dump.contains("20240101000000_person") && dump.contains("20240102000000_age"),
        "dump should list both applied versions: {dump}"
    );
    assert!(dump.contains("define"), "dump should include schema body");

    runner.rollback().await.expect("rollback latest");
    assert!(
        runner.status(true).await.expect("status after rollback"),
        "one migration should be pending after rollback"
    );
    runner.dump().await.expect("dump after rollback");
    let dump2 = std::fs::read_to_string(&schema).expect("read dump2");
    assert!(
        dump2.contains("20240101000000_person"),
        "first version should remain in dump header"
    );
    assert!(
        !dump2.contains("20240102000000_age"),
        "rolled-back version must not appear as applied: {dump2}"
    );

    runner.migrate().await.expect("re-migrate age");

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
        "failed migration must not be recorded"
    );

    runner.drop().await.expect("drop");
    runner.create().await.expect("recreate after drop");
    runner.drop().await.expect("final drop");
}

#[tokio::test]
async fn up_creates_database_and_applies() {
    let typedb = shared_typedb().await;
    let url = typedb.url_for(&unique_db("tqlmate_up"));
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
    let typedb = shared_typedb().await;
    let url = typedb.url_for(&unique_db("tqlmate_empty"));
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
