use aws_credential_types::Credentials;
use aws_sdk_s3::config::{BehaviorVersion, Region};
use aws_sdk_s3::primitives::ByteStream;
use aws_sdk_s3::Client as S3Client;

use crate::config::Config;

pub fn create_s3_client(config: &Config) -> S3Client {
    let creds = Credentials::new(
        &config.s3_access_key,
        &config.s3_secret_key,
        None,
        None,
        "env",
    );

    let s3_config = aws_sdk_s3::Config::builder()
        .behavior_version(BehaviorVersion::latest())
        .region(Region::new(config.s3_region.clone()))
        .endpoint_url(&config.s3_endpoint)
        .credentials_provider(creds)
        .force_path_style(true)
        .build();

    S3Client::from_conf(s3_config)
}

pub async fn upload_file(
    client: &S3Client,
    bucket: &str,
    key: &str,
    bytes: Vec<u8>,
    content_type: &str,
) -> Result<(), String> {
    client
        .put_object()
        .bucket(bucket)
        .key(key)
        .body(ByteStream::from(bytes))
        .content_type(content_type)
        .acl(aws_sdk_s3::types::ObjectCannedAcl::PublicRead)
        .send()
        .await
        .map_err(|e| format!("S3 upload failed: {}", e))?;
    Ok(())
}

pub async fn delete_file(client: &S3Client, bucket: &str, key: &str) -> Result<(), String> {
    client
        .delete_object()
        .bucket(bucket)
        .key(key)
        .send()
        .await
        .map_err(|e| format!("S3 delete failed: {}", e))?;
    Ok(())
}

pub fn cdn_url(config: &Config, key: &str) -> String {
    format!("{}/{}", config.s3_cdn_url.trim_end_matches('/'), key)
}
