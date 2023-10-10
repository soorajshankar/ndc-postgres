//! Internal Configuration and state for our connector.
use tracing::{info_span, Instrument};

use ndc_sdk::connector;
use ndc_sdk::models::secret_or_literal_reference;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sqlx::postgres::PgConnection;
use sqlx::{Connection, Executor, Row};

use query_engine_metadata::metadata;

const CURRENT_VERSION: u32 = 1;

/// Initial configuration, just enough to connect to a database and elaborate a full
/// 'Configuration'.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize, JsonSchema)]
pub struct RawConfiguration {
    // Which version of the configuration format are we using
    pub version: u32,
    // Connection string for a Postgres-compatible database
    pub connection_uris: ConnectionUris,
    #[serde(skip_serializing_if = "PoolSettings::is_default")]
    #[serde(default)]
    pub pool_settings: PoolSettings,
    #[serde(default)]
    pub metadata: metadata::Metadata,
    #[serde(default)]
    pub aggregate_functions: metadata::AggregateFunctions,
    /// Schemas which are excluded from introspection. The default setting will exclude the
    /// internal schemas of Postgres, Citus, Cockroach, and the PostGIS extension.
    #[serde(default = "default_excluded_schemas")]
    pub excluded_schemas: Vec<String>,
}

fn default_excluded_schemas() -> Vec<String> {
    vec![
        // From Postgres itself
        "information_schema".to_string(),
        "pg_catalog".to_string(),
        // From PostGIS
        "tiger".to_string(),
        // From CockroachDB
        "crdb_internal".to_string(),
        // From Citus
        "columnar".to_string(),
        "columnar_internal".to_string(),
    ]
}

/// User configuration, elaborated from a 'RawConfiguration'.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize, JsonSchema)]
pub struct Configuration {
    pub config: RawConfiguration,
}

/// Type that accept both a single value and a list of values. Allows for a simpler format when a
/// single value is the common case.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum SingleOrList<T> {
    Single(T),
    List(Vec<T>),
}

impl<T: Clone> SingleOrList<T> {
    fn is_empty(&self) -> bool {
        match self {
            SingleOrList::Single(_) => false,
            SingleOrList::List(l) => l.is_empty(),
        }
    }

    fn to_vec(&self) -> Vec<T> {
        match self {
            SingleOrList::Single(s) => vec![s.clone()],
            SingleOrList::List(l) => l.clone(),
        }
    }
}

impl<'a, T> IntoIterator for &'a SingleOrList<T> {
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        match self {
            SingleOrList::Single(s) => std::slice::from_ref(s).iter(),
            SingleOrList::List(l) => l.iter(),
        }
    }
}

// In the user facing configuration, the connection string can either be a literal or a reference
// to a secret, so we advertize either in the JSON schema. However, when building the configuration,
// we expect the metadata build service to have resolved the secret reference so we deserialize
// only to a String.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize, JsonSchema)]
pub struct ConnectionUri(#[schemars(schema_with = "secret_or_literal_reference")] pub String);

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize, JsonSchema)]
pub struct ConnectionUris(pub SingleOrList<ConnectionUri>);

pub fn single_connection_uri(connection_uri: String) -> ConnectionUris {
    ConnectionUris(SingleOrList::Single(ConnectionUri(connection_uri)))
}

impl RawConfiguration {
    pub fn empty() -> Self {
        Self {
            version: CURRENT_VERSION,
            connection_uris: ConnectionUris(SingleOrList::List(vec![])),
            pool_settings: PoolSettings::default(),
            metadata: metadata::Metadata::default(),
            aggregate_functions: metadata::AggregateFunctions::default(),
            excluded_schemas: default_excluded_schemas(),
        }
    }
}

/// Settings for the PostgreSQL connection pool
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PoolSettings {
    /// maximum number of pool connections
    #[serde(default = "max_connection_default")]
    pub max_connections: u32,
    /// timeout for acquiring a connection from the pool (seconds)
    #[serde(default = "pool_timeout_default")]
    pub pool_timeout: u64,
    /// idle timeout for releasing a connection from the pool (seconds)
    #[serde(default = "idle_timeout_default")]
    pub idle_timeout: Option<u64>,
    /// maximum lifetime for an individual connection (seconds)
    #[serde(default = "connection_lifetime_default")]
    pub connection_lifetime: Option<u64>,
}

impl PoolSettings {
    fn is_default(&self) -> bool {
        *self == PoolSettings::default()
    }
}

/// <https://hasura.io/docs/latest/api-reference/syntax-defs/#pgpoolsettings>
impl Default for PoolSettings {
    fn default() -> PoolSettings {
        PoolSettings {
            max_connections: 50,
            pool_timeout: 30,
            idle_timeout: Some(180),
            connection_lifetime: Some(600),
        }
    }
}

// for serde default //
fn max_connection_default() -> u32 {
    PoolSettings::default().max_connections
}
fn pool_timeout_default() -> u64 {
    PoolSettings::default().pool_timeout
}
fn idle_timeout_default() -> Option<u64> {
    PoolSettings::default().idle_timeout
}
fn connection_lifetime_default() -> Option<u64> {
    PoolSettings::default().connection_lifetime
}

/// Validate the user configuration.
pub async fn validate_raw_configuration(
    rawconfiguration: &RawConfiguration,
) -> Result<Configuration, connector::ValidateError> {
    if rawconfiguration.version != 1 {
        return Err(connector::ValidateError::ValidateError(vec![
            connector::InvalidRange {
                path: vec![connector::KeyOrIndex::Key("version".into())],
                message: format!(
                    "invalid configuration version, expected 1, got {0}",
                    rawconfiguration.version
                ),
            },
        ]));
    }

    match &rawconfiguration.connection_uris {
        ConnectionUris(urls) if urls.is_empty() => {
            Err(connector::ValidateError::ValidateError(vec![
                connector::InvalidRange {
                    path: vec![connector::KeyOrIndex::Key("connection_uris".into())],
                    message: "At least one database url must be specified".to_string(),
                },
            ]))
        }
        _ => Ok(()),
    }?;

    Ok(Configuration {
        config: rawconfiguration.clone(),
    })
}

/// Select the first available connection uri.
pub fn select_first_connection_url(ConnectionUris(urls): &ConnectionUris) -> String {
    urls.to_vec()[0].clone().0
}

/// Select a single connection uri to use.
///
/// Currently we simply select the first specified connection uri.
///
/// Eventually we want to support load-balancing between multiple read-replicas,
/// and then we'll be passing the full list of connection uris to the connection pool.
pub fn select_connection_url(ConnectionUris(urls): &ConnectionUris) -> String {
    urls.to_vec()[0].clone().0
}

/// Construct the deployment configuration by introspecting the database.
pub async fn configure(
    args: &RawConfiguration,
    configuration_query: &str,
) -> Result<RawConfiguration, connector::UpdateConfigurationError> {
    let url = select_first_connection_url(&args.connection_uris);

    let mut connection = PgConnection::connect(url.as_str())
        .instrument(info_span!("Connect to database"))
        .await
        .map_err(|e| connector::UpdateConfigurationError::Other(e.into()))?;

    let query = sqlx::query(configuration_query).bind(args.excluded_schemas.clone());

    let row = connection
        .fetch_one(query)
        .instrument(info_span!("Run introspection query"))
        .await
        .map_err(|e| connector::UpdateConfigurationError::Other(e.into()))?;

    let (tables, aggregate_functions) = async {
        let tables: metadata::TablesInfo = serde_json::from_value(row.get(0))
            .map_err(|e| connector::UpdateConfigurationError::Other(e.into()))?;

        let aggregate_functions: metadata::AggregateFunctions = serde_json::from_value(row.get(1))
            .map_err(|e| connector::UpdateConfigurationError::Other(e.into()))?;

        Ok((tables, aggregate_functions))
    }
    .instrument(info_span!("Decode introspection result"))
    .await?;

    Ok(RawConfiguration {
        version: 1,
        connection_uris: args.connection_uris.clone(),
        pool_settings: args.pool_settings.clone(),
        metadata: metadata::Metadata {
            tables,
            native_queries: args.metadata.native_queries.clone(),
        },
        aggregate_functions,
        excluded_schemas: args.excluded_schemas.clone(),
    })
}
