//! Tests that configuration generation has not changed.
//!
//! If you have changed it intentionally, run `just generate-chinook-configuration`.

pub mod common;

use std::fs;

use similar_asserts::assert_eq;

use tests_common::deployment::helpers::get_path_from_project_root;

const CONFIGURATION_QUERY: &str = include_str!("../../ndc-postgres/src/configuration.sql");

#[tokio::test]
async fn test_configure() {
    let args = ndc_postgres::configuration::RawConfiguration {
        connection_uris: ndc_postgres::configuration::single_connection_uri(
            common::POSTGRESQL_CONNECTION_STRING.to_string(),
        ),
        ..ndc_postgres::configuration::RawConfiguration::empty()
    };

    let expected_value: serde_json::Value = {
        let file = fs::File::open(get_path_from_project_root(common::CHINOOK_DEPLOYMENT_PATH))
            .expect("fs::File::open");
        let mut result: serde_json::Value =
            serde_json::from_reader(file).expect("serde_json::from_reader");

        // We need to ignore certain properties in the configuration file
        // because they cannot be generated from the database.

        // 1. the connection pool settings
        result.as_object_mut().unwrap().remove("pool_settings");
        // 2. native queries
        result["metadata"]["native_queries"]
            .as_object_mut()
            .unwrap()
            .clear();
        result
    };

    let actual = ndc_postgres::configuration::configure(args, CONFIGURATION_QUERY)
        .await
        .expect("configuration::configure");

    let actual_value = serde_json::to_value(actual).expect("serde_json::to_value");

    assert_eq!(expected_value, actual_value);
}
