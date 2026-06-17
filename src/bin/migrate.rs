use anyhow::Context;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    let database_url = std::env::var("DATABASE_URL").context("DATABASE_URL must be set")?;
    let pool = dtr::db::connect(&database_url).await?;
    dtr::db::migrate(&pool).await?;
    println!("migrations applied");
    Ok(())
}
