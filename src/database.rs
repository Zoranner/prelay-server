#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DatabaseKind {
    Sqlite,
    Postgres,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DatabaseConfig {
    url: String,
    kind: DatabaseKind,
    max_connections: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DatabaseConfigError {
    MissingUrl,
    UnsupportedScheme { scheme: String },
    InvalidMaxConnections { value: String },
}

pub enum DatabaseError {
    Connect { kind: DatabaseKind },
}

impl std::fmt::Display for DatabaseError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Connect { kind } => write!(formatter, "failed to connect to {kind} database"),
        }
    }
}

impl std::fmt::Debug for DatabaseError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(self, formatter)
    }
}

impl std::error::Error for DatabaseError {}

impl std::fmt::Display for DatabaseKind {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Sqlite => formatter.write_str("SQLite"),
            Self::Postgres => formatter.write_str("PostgreSQL"),
        }
    }
}

impl std::fmt::Display for DatabaseConfigError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingUrl => formatter.write_str("DATABASE_URL must be set"),
            Self::UnsupportedScheme { scheme } => {
                write!(formatter, "unsupported database URL scheme: {scheme}")
            }
            Self::InvalidMaxConnections { value } => write!(
                formatter,
                "DATABASE_MAX_CONNECTIONS must be a non-zero unsigned integer, got: {value}"
            ),
        }
    }
}

impl std::error::Error for DatabaseConfigError {}

impl DatabaseConfig {
    pub fn from_url(value: &str) -> Result<Self, DatabaseConfigError> {
        let url = value.trim();
        if url.is_empty() {
            return Err(DatabaseConfigError::MissingUrl);
        }

        let scheme = url
            .split_once(':')
            .map(|(scheme, _)| scheme.to_ascii_lowercase())
            .ok_or_else(|| DatabaseConfigError::UnsupportedScheme {
                scheme: "<missing>".to_owned(),
            })?;
        let (kind, max_connections) = match scheme.as_str() {
            "sqlite" => (DatabaseKind::Sqlite, 1),
            "postgres" | "postgresql" => (DatabaseKind::Postgres, 10),
            _ => return Err(DatabaseConfigError::UnsupportedScheme { scheme }),
        };

        Ok(Self {
            url: url.to_string(),
            kind,
            max_connections,
        })
    }

    pub fn from_environment() -> Result<Self, DatabaseConfigError> {
        let url = std::env::var("DATABASE_URL").map_err(|_| DatabaseConfigError::MissingUrl)?;
        let mut config = Self::from_url(&url)?;

        if config.kind == DatabaseKind::Postgres {
            if let Ok(value) = std::env::var("DATABASE_MAX_CONNECTIONS") {
                config.max_connections = value
                    .parse()
                    .ok()
                    .filter(|limit| *limit > 0)
                    .ok_or(DatabaseConfigError::InvalidMaxConnections { value })?;
            }
        }

        Ok(config)
    }

    pub fn url(&self) -> &str {
        &self.url
    }

    pub const fn kind(&self) -> DatabaseKind {
        self.kind
    }

    pub const fn max_connections(&self) -> u32 {
        self.max_connections
    }
}

pub async fn connect(config: &DatabaseConfig) -> Result<DatabaseConnection, DatabaseError> {
    let mut options = ConnectOptions::new(config.url().to_owned());
    options
        .max_connections(config.max_connections())
        .sqlx_logging(false);
    Database::connect(options)
        .await
        .map_err(|_: DbErr| DatabaseError::Connect {
            kind: config.kind(),
        })
}
use sea_orm::{ConnectOptions, Database, DatabaseConnection, DbErr};
