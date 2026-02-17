use std::time::Duration;

use aws_sdk_s3::Client as S3Client;
use sqlx::PgPool;
use tokio::sync::broadcast;

use crate::db;
use crate::s3;

pub async fn run_cleanup_worker(
    pool: PgPool,
    s3_client: S3Client,
    s3_bucket: String,
    mut shutdown: broadcast::Receiver<()>,
) {
    let mut interval = tokio::time::interval(Duration::from_secs(3600));

    tracing::info!("Cleanup worker started (runs every hour)");

    loop {
        tokio::select! {
            _ = interval.tick() => {
                cleanup_expired(&pool, &s3_client, &s3_bucket).await;
            }
            _ = shutdown.recv() => {
                tracing::info!("Cleanup worker shutting down");
                break;
            }
        }
    }
}

async fn cleanup_expired(pool: &PgPool, s3_client: &S3Client, s3_bucket: &str) {
    let expired = match db::find_expired(pool).await {
        Ok(records) => records,
        Err(e) => {
            tracing::error!("Failed to query expired media: {}", e);
            return;
        }
    };

    if expired.is_empty() {
        tracing::debug!("No expired media to clean up");
        return;
    }

    tracing::info!("Cleaning up {} expired media records", expired.len());

    for record in &expired {
        if let Err(e) = s3::delete_file(s3_client, s3_bucket, &record.s3_key).await {
            tracing::error!("Failed to delete S3 object {}: {}", record.s3_key, e);
            continue;
        }

        if let Err(e) = db::delete_by_id(pool, record.id).await {
            tracing::error!("Failed to delete DB record {}: {}", record.id, e);
        } else {
            tracing::info!("Cleaned up expired media: {} ({})", record.s3_key, record.id);
        }
    }
}
