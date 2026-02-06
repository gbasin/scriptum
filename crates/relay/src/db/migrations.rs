use anyhow::{Context, Result};
use sqlx::{migrate::Migrator, postgres::PgPool};

pub static MIGRATOR: Migrator = sqlx::migrate!("./src/db/migrations");

pub async fn run_migrations(pool: &PgPool) -> Result<()> {
    MIGRATOR.run(pool).await.context("failed to apply relay postgres migrations")
}
