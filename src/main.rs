#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dtr::run().await
}
