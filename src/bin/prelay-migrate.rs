use std::path::Path;

use prelay_server::{
    database::{connect, DatabaseConfig},
    migration::apply_all,
};

fn load_environment_file(path: &Path) -> anyhow::Result<()> {
    match dotenvy::from_path(path) {
        Ok(()) => Ok(()),
        Err(error) if error.not_found() => Ok(()),
        Err(error) => Err(error.into()),
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    load_environment_file(Path::new(".env"))?;
    let database_config = DatabaseConfig::from_environment()?;
    let db = connect(&database_config).await?;
    apply_all(&db).await?;
    Ok(())
}
