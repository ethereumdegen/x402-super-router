use chrono::{DateTime, Utc};
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Debug, Clone, sqlx::FromRow)]
#[allow(dead_code)]
pub struct MediaRecord {
    pub id: Uuid,
    pub endpoint_path: String,
    pub prompt: String,
    pub prompt_hash: String,
    pub s3_key: String,
    pub s3_url: String,
    pub media_type: String,
    pub file_size_bytes: i64,
    pub payer_address: Option<String>,
    pub payment_tx: Option<String>,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

pub async fn create_pool(database_url: &str) -> PgPool {
    PgPoolOptions::new()
        .max_connections(10)
        .connect(database_url)
        .await
        .expect("Failed to connect to database")
}

pub async fn find_by_prompt_hash(
    pool: &PgPool,
    prompt_hash: &str,
    endpoint_path: &str,
) -> Result<Option<MediaRecord>, sqlx::Error> {
    sqlx::query_as::<_, MediaRecord>(
        "SELECT * FROM generated_media WHERE prompt_hash = $1 AND endpoint_path = $2 AND expires_at > NOW() LIMIT 1",
    )
    .bind(prompt_hash)
    .bind(endpoint_path)
    .fetch_optional(pool)
    .await
}

#[allow(clippy::too_many_arguments)]
pub async fn insert_media(
    pool: &PgPool,
    endpoint_path: &str,
    prompt: &str,
    prompt_hash: &str,
    s3_key: &str,
    s3_url: &str,
    media_type: &str,
    file_size_bytes: i64,
    payer_address: Option<&str>,
    payment_tx: Option<&str>,
) -> Result<Uuid, sqlx::Error> {
    let rec = sqlx::query_scalar::<_, Uuid>(
        "INSERT INTO generated_media (endpoint_path, prompt, prompt_hash, s3_key, s3_url, media_type, file_size_bytes, payer_address, payment_tx)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
         RETURNING id",
    )
    .bind(endpoint_path)
    .bind(prompt)
    .bind(prompt_hash)
    .bind(s3_key)
    .bind(s3_url)
    .bind(media_type)
    .bind(file_size_bytes)
    .bind(payer_address)
    .bind(payment_tx)
    .fetch_one(pool)
    .await?;
    Ok(rec)
}

pub async fn find_expired(pool: &PgPool) -> Result<Vec<MediaRecord>, sqlx::Error> {
    sqlx::query_as::<_, MediaRecord>("SELECT * FROM generated_media WHERE expires_at <= NOW()")
        .fetch_all(pool)
        .await
}

pub async fn delete_by_id(pool: &PgPool, id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM generated_media WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}
