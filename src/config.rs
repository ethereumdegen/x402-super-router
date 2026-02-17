use std::env;

#[derive(Debug, Clone)]
pub struct Config {
    pub test_mode: bool,
    pub port: u16,
    pub facilitator_url: String,
    pub facilitator_signer: String,
    pub wallet_address: String,
    pub payment_network: String,
    pub payment_token_address: String,
    pub payment_token_symbol: String,
    pub payment_token_decimals: u8,
    pub payment_token_name: String,
    pub payment_token_version: String,
    pub fal_key: String,
    pub public_url: String,
    pub endpoints_config_path: String,
    pub s3_endpoint: String,
    pub s3_bucket: String,
    pub s3_region: String,
    pub s3_access_key: String,
    pub s3_secret_key: String,
    pub s3_cdn_url: String,
    pub database_url: String,
}

impl Config {
    pub fn from_env() -> Self {
        let test_mode = env::var("TEST_MODE")
            .map(|v| v == "true" || v == "1")
            .unwrap_or(false);

        Self {
            test_mode,
            port: env::var("PORT")
                .unwrap_or_else(|_| "3402".to_string())
                .parse()
                .expect("PORT must be a valid number"),
            facilitator_url: env::var("FACILITATOR_URL")
                .unwrap_or_else(|_| "https://facilitator.x402.org".to_string()),
            facilitator_signer: env::var("FACILITATOR_SIGNER")
                .unwrap_or_else(|_| if test_mode { String::new() } else { panic!("FACILITATOR_SIGNER must be set") }),
            wallet_address: env::var("WALLET_ADDRESS")
                .unwrap_or_else(|_| if test_mode { String::new() } else { panic!("WALLET_ADDRESS must be set") }),
            payment_network: env::var("PAYMENT_NETWORK")
                .unwrap_or_else(|_| "base".to_string()),
            payment_token_address: env::var("PAYMENT_TOKEN_ADDRESS")
                .unwrap_or_else(|_| "0x587Cd533F418825521f3A1daa7CCd1E7339A1B07".to_string()),
            payment_token_symbol: env::var("PAYMENT_TOKEN_SYMBOL")
                .unwrap_or_else(|_| "STARKBOT".to_string()),
            payment_token_decimals: env::var("PAYMENT_TOKEN_DECIMALS")
                .unwrap_or_else(|_| "18".to_string())
                .parse()
                .expect("PAYMENT_TOKEN_DECIMALS must be a valid number"),
            payment_token_name: env::var("PAYMENT_TOKEN_NAME")
                .unwrap_or_else(|_| "StarkBot".to_string()),
            payment_token_version: env::var("PAYMENT_TOKEN_VERSION")
                .unwrap_or_else(|_| "1".to_string()),
            fal_key: env::var("FAL_KEY").expect("FAL_KEY must be set"),
            public_url: env::var("PUBLIC_URL")
                .unwrap_or_else(|_| "http://localhost:3402".to_string()),
            endpoints_config_path: env::var("ENDPOINTS_CONFIG")
                .unwrap_or_else(|_| "endpoints.ron".to_string()),
            s3_endpoint: env::var("S3_ENDPOINT").expect("S3_ENDPOINT must be set"),
            s3_bucket: env::var("S3_BUCKET").expect("S3_BUCKET must be set"),
            s3_region: env::var("S3_REGION").unwrap_or_else(|_| "nyc3".to_string()),
            s3_access_key: env::var("S3_ACCESS_KEY").expect("S3_ACCESS_KEY must be set"),
            s3_secret_key: env::var("S3_SECRET_KEY").expect("S3_SECRET_KEY must be set"),
            s3_cdn_url: env::var("S3_CDN_URL").unwrap_or_else(|_| {
                let bucket = env::var("S3_BUCKET").expect("S3_BUCKET must be set");
                let region = env::var("S3_REGION").unwrap_or_else(|_| "nyc3".to_string());
                format!("https://{}.{}.digitaloceanspaces.com", bucket, region)
            }),
            database_url: env::var("DATABASE_URL").expect("DATABASE_URL must be set"),
        }
    }
}
