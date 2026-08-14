//! AWS integration for cloud auto-provisioning.
//!
//! Phase 1 only: verify the user's AWS credentials (STS GetCallerIdentity) and
//! list their S3 buckets, so the setup UI can confirm the connection and offer
//! a bucket picker. Later phases (EC2 provisioning, S3 mount, teardown) build
//! on the same credentials.

use std::sync::Once;

// reqwest (rustls with ring) and the AWS SDK's default rustls connector can
// both expect a process-default CryptoProvider. Install ring's default once so
// neither path panics with "no process-level CryptoProvider available".
static CRYPTO_INIT: Once = Once::new();
fn ensure_crypto() {
    CRYPTO_INIT.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

fn region_or_default(region: &str) -> String {
    let r = region.trim();
    if r.is_empty() {
        "us-east-1".to_string()
    } else {
        r.to_string()
    }
}

/// Flatten an error and its source chain into a single readable message.
/// AWS `SdkError`'s own `Display` is terse ("service error"); the useful detail
/// (e.g. "The security token included in the request is invalid") lives in the
/// source chain.
fn fmt_err<E: std::error::Error>(e: E) -> String {
    let mut msg = e.to_string();
    let mut src = e.source();
    while let Some(s) = src {
        let s_str = s.to_string();
        if !msg.contains(&s_str) {
            msg.push_str(": ");
            msg.push_str(&s_str);
        }
        src = s.source();
    }
    msg
}

/// Verify AWS credentials via STS GetCallerIdentity. Returns the account ID.
pub async fn verify_credentials(
    access_key: &str,
    secret_key: &str,
    region: &str,
) -> Result<String, String> {
    ensure_crypto();
    let conf = aws_sdk_sts::config::Builder::new()
        .behavior_version(aws_sdk_sts::config::BehaviorVersion::latest())
        .region(aws_sdk_sts::config::Region::new(region_or_default(region)))
        .credentials_provider(aws_sdk_sts::config::Credentials::new(
            access_key.trim(),
            secret_key.trim(),
            None,
            None,
            "autopipe",
        ))
        .build();
    let client = aws_sdk_sts::Client::from_conf(conf);
    let out = client
        .get_caller_identity()
        .send()
        .await
        .map_err(fmt_err)?;
    Ok(out.account().unwrap_or_default().to_string())
}

/// List the account's S3 bucket names using the given credentials.
pub async fn list_buckets(
    access_key: &str,
    secret_key: &str,
    region: &str,
) -> Result<Vec<String>, String> {
    ensure_crypto();
    let conf = aws_sdk_s3::config::Builder::new()
        .behavior_version(aws_sdk_s3::config::BehaviorVersion::latest())
        .region(aws_sdk_s3::config::Region::new(region_or_default(region)))
        .credentials_provider(aws_sdk_s3::config::Credentials::new(
            access_key.trim(),
            secret_key.trim(),
            None,
            None,
            "autopipe",
        ))
        .build();
    let client = aws_sdk_s3::Client::from_conf(conf);
    let out = client.list_buckets().send().await.map_err(fmt_err)?;
    let mut names: Vec<String> = out
        .buckets()
        .iter()
        .filter_map(|b| b.name().map(|s| s.to_string()))
        .collect();
    names.sort();
    Ok(names)
}
