use sqlx::migrate::Migrator;
use sqlx::postgres::PgPoolOptions;
use std::env;
use std::path::Path;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

    let database_url = env::var("DATABASE_URL").expect("DATABASE_URL must be set");

    println!("Connecting to database...");
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .connect(&database_url)
        .await?;

    println!("Running migrations...");
    let mut migrator = Migrator::new(Path::new("./migrations")).await?;
    migrator.ignore_missing = true;
    migrator.run(&pool).await?;

    println!("Migrations completed successfully!");
    Ok(())
}
