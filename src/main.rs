#[tokio::main]
async fn main() -> anyhow::Result<()> {
    prelay_server::bootstrap::serve().await
}
