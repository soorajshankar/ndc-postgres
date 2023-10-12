//! Tests that configuration generation has not changed.
//!
//! If you have changed it intentionally, run `just generate-chinook-configuration`.

pub mod common;

use std::fs;

use similar_asserts::assert_eq;

use ndc_postgres::configuration;

use tests_common::deployment::helpers::get_path_from_project_root;

const CONFIGURATION_QUERY: &str = include_str!("../../ndc-postgres/src/configuration.sql");

#[tokio::test]
async fn test_configure() {
    let expected_value: serde_json::Value = {
        let file = fs::File::open(get_path_from_project_root(common::CHINOOK_DEPLOYMENT_PATH))
            .expect("fs::File::open");
        let result: serde_json::Value =
            serde_json::from_reader(file).expect("serde_json::from_reader");

        result
    };

    let mut args: configuration::RawConfiguration = serde_json::from_value(expected_value.clone())
        .expect("Unable to deserialize as RawConfiguration");

    args.connection_uri = configuration::ConnectionUri::Uri(configuration::ResolvedSecret(
        common::POSTGRESQL_CONNECTION_STRING.to_string(),
    ));

    let actual = configuration::configure(args, CONFIGURATION_QUERY)
        .await
        .expect("configuration::configure");

    let actual_value = serde_json::to_value(actual).expect("serde_json::to_value");

    assert_eq!(expected_value, actual_value);
}