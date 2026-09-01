//! Placeholder when `typedb-docker` is disabled (`--no-default-features`).
//! Real Docker-backed tests live in `integration.rs` under `cfg(feature = "typedb-docker")`.

#![cfg(not(feature = "typedb-docker"))]

#[test]
#[ignore = "enable feature typedb-docker (default) to run TypeDB Docker integration tests"]
fn typedb_docker_integration_disabled() {}
