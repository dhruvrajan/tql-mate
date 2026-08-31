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

fn opts(url: TypeDbUrl, migrations: PathBuf) -> Opts {
    Opts {
        url,
        migrations_dir: migrations,
        schema_file: std::env::temp_dir().join("tqlmate-test-schema.tql"),
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

#[tokio::test]
async fn migrate_rollback_and_failed_up() {
    let mut url = typedb_url();
    if !server_up(&url).await {
        if require_typedb() {
            panic!(
                "TypeDB required but not reachable at {} (set TYPEDB_URL)",
                url.address()
            );
        }
        eprintln!("skip: TypeDB not reachable at {}", url.address());
        return;
    }

    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis();
    url.database = format!("tqlmate_{millis}");

    let tmp = tempfile::tempdir().unwrap();
    let migrations = tmp.path().join("migrations");
    std::fs::create_dir_all(&migrations).unwrap();

    std::fs::write(
        migrations.join("20240101000000_person.tql"),
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
    )
    .unwrap();
    std::fs::write(
        migrations.join("20240102000000_age.tql"),
        "-- migrate:up\n\
         define\n\
           entity person, owns age;\n\
           attribute age, value integer;\n\
         \n\
         -- migrate:down\n\
         undefine\n\
           owns age from person;\n\
           age;\n",
    )
    .unwrap();

    let mut runner = Runner::new(opts(url, migrations.clone()));
    runner.drop().await.ok();
    runner.create().await.expect("create");
    runner.migrate().await.expect("migrate");
    assert!(!runner.status(true).await.expect("status"));

    runner.rollback().await.expect("rollback");
    assert!(runner.status(true).await.expect("status after rollback"));

    runner.dump().await.expect("dump");
    runner.migrate().await.expect("re-migrate");

    std::fs::write(
        migrations.join("20240103000000_bad.tql"),
        "-- migrate:up\ndefine\n  this is not valid typeql !!!\n\n-- migrate:down\n\n",
    )
    .unwrap();
    assert!(runner.migrate().await.is_err());
    assert!(
        runner.status(true).await.expect("status after fail"),
        "failed migration must not be recorded"
    );

    runner.drop().await.expect("drop");
}
