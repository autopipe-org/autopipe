//! AWS integration for cloud auto-provisioning.
//!
//! Phase 1 only: verify the user's AWS credentials (STS GetCallerIdentity) and
//! list their S3 buckets, so the setup UI can confirm the connection and offer
//! a bucket picker. Later phases (EC2 provisioning, S3 mount, teardown) build
//! on the same credentials.

use base64::Engine as _;
use std::sync::Once;
use std::time::Duration;

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

/// Verify AWS credentials via STS GetCallerIdentity. Returns (account_id,
/// iam_user_name). The user name is parsed from the caller ARN (last path
/// segment of `.../user/NAME`) and is used to pre-fill the CloudFormation
/// permission-setup stack; it is empty for the root user.
pub async fn verify_credentials(
    access_key: &str,
    secret_key: &str,
    region: &str,
) -> Result<(String, String), String> {
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
    let account = out.account().unwrap_or_default().to_string();
    let arn = out.arn().unwrap_or_default();
    // "arn:aws:iam::123:user/qoka" -> "qoka"; root ARN has no '/' -> empty.
    let username = if arn.contains('/') {
        arn.rsplit('/').next().unwrap_or_default().to_string()
    } else {
        String::new()
    };
    Ok((account, username))
}

/// Instance profile (created by the CloudFormation setup) that gives the VM
/// keyless S3 access. Attached at launch so pipelines can mount S3 buckets.
pub const INSTANCE_PROFILE_NAME: &str = "autopipe-vm-profile";

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

/// List one "folder level" of an S3 bucket for the input file explorer:
/// subfolders (common prefixes) and files (objects) directly under `prefix`.
/// Returns (folders, files) where each file is (key, size_bytes).
pub async fn list_objects(
    access_key: &str,
    secret_key: &str,
    region: &str,
    bucket: &str,
    prefix: &str,
) -> Result<(Vec<String>, Vec<(String, i64)>), String> {
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

    let mut folders: Vec<String> = Vec::new();
    let mut files: Vec<(String, i64)> = Vec::new();
    let mut continuation: Option<String> = None;
    loop {
        let mut req = client
            .list_objects_v2()
            .bucket(bucket)
            .delimiter("/")
            .prefix(prefix);
        if let Some(tok) = continuation.take() {
            req = req.continuation_token(tok);
        }
        let out = req.send().await.map_err(fmt_err)?;
        for cp in out.common_prefixes() {
            if let Some(p) = cp.prefix() {
                folders.push(p.to_string());
            }
        }
        for obj in out.contents() {
            if let Some(k) = obj.key() {
                if k == prefix {
                    continue; // the folder-marker object itself
                }
                files.push((k.to_string(), obj.size().unwrap_or(0)));
            }
        }
        if out.is_truncated().unwrap_or(false) {
            continuation = out.next_continuation_token().map(|s| s.to_string());
            if continuation.is_none() {
                break;
            }
        } else {
            break;
        }
    }
    folders.sort();
    files.sort();
    Ok((folders, files))
}

// ── EC2 auto-provisioning (Phase 2) ──────────────────────────────────

/// First-boot install script (matches AWS.md §7): curl bootstrap + AutoPipe
/// setup.sh (Docker/Git/gh) + docker group for ubuntu + rclone + a readiness
/// marker AutoPipe polls before using the VM.
const USER_DATA: &str = r#"#!/bin/bash
set -e
# Broaden SSH algorithm support so libssh2-based clients — notably AutoPipe on
# Windows, which uses the WinCNG backend and cannot negotiate the newest
# key-exchange/host-key algorithms — can complete the handshake to this modern
# OpenSSH server. The '+' keeps the secure defaults and merely re-adds fallbacks.
cat >/etc/ssh/sshd_config.d/50-autopipe-compat.conf <<'SSHD'
HostKeyAlgorithms +ssh-rsa,rsa-sha2-256,rsa-sha2-512
PubkeyAcceptedAlgorithms +ssh-rsa,rsa-sha2-256,rsa-sha2-512
KexAlgorithms +diffie-hellman-group14-sha256,diffie-hellman-group-exchange-sha256,diffie-hellman-group14-sha1
SSHD
if sshd -t 2>/dev/null; then
  systemctl restart ssh 2>/dev/null || systemctl restart sshd 2>/dev/null || true
else
  rm -f /etc/ssh/sshd_config.d/50-autopipe-compat.conf
fi
command -v curl  >/dev/null || { apt-get update -qq && apt-get install -y -qq curl ca-certificates; }
curl -fsSL https://download.autopipe.org/setup.sh | bash
id -nG ubuntu | grep -qw docker || usermod -aG docker ubuntu
command -v rclone >/dev/null || curl https://rclone.org/install.sh | bash
mkdir -p /var/lib/autopipe && touch /var/lib/autopipe/ready
"#;

pub struct ProvisionResult {
    pub instance_id: String,
    pub sg_id: String,
    pub key_name: String,
    pub key_path: String,
    pub public_ip: String,
}

fn short_id() -> String {
    let s = uuid::Uuid::new_v4().simple().to_string();
    s[..8].to_string()
}

fn ec2_client(access_key: &str, secret_key: &str, region: &str) -> aws_sdk_ec2::Client {
    let conf = aws_sdk_ec2::config::Builder::new()
        .behavior_version(aws_sdk_ec2::config::BehaviorVersion::latest())
        .region(aws_sdk_ec2::config::Region::new(region_or_default(region)))
        .credentials_provider(aws_sdk_ec2::config::Credentials::new(
            access_key.trim(),
            secret_key.trim(),
            None,
            None,
            "autopipe",
        ))
        .build();
    aws_sdk_ec2::Client::from_conf(conf)
}

/// The caller's own public IP, used to scope the security group's SSH rule.
async fn get_public_ip() -> Result<String, String> {
    let resp = reqwest::get("https://checkip.amazonaws.com")
        .await
        .map_err(|e| format!("could not detect your public IP: {e}"))?;
    let txt = resp
        .text()
        .await
        .map_err(|e| format!("could not detect your public IP: {e}"))?;
    Ok(txt.trim().to_string())
}

/// Provision an EC2 VM in the user's account: pick the latest Ubuntu AMI, create
/// a key pair + security group (SSH from the caller's IP), launch with the
/// install user-data, and wait until it is running with a public IP.
pub async fn provision_vm(
    access_key: &str,
    secret_key: &str,
    region: &str,
    instance_type: &str,
    key_dir: &str,
) -> Result<ProvisionResult, String> {
    use aws_sdk_ec2::types::{
        Filter, IamInstanceProfileSpecification, InstanceStateName, InstanceType, IpPermission,
        IpRange, ResourceType, Tag, TagSpecification,
    };

    ensure_crypto();
    let client = ec2_client(access_key, secret_key, region);
    let name = format!("autopipe-{}", short_id());

    // 1. Latest Canonical Ubuntu AMI (x86_64, EBS, HVM SSD).
    let imgs = client
        .describe_images()
        .owners("099720109477")
        .filters(
            Filter::builder()
                .name("name")
                .values("ubuntu/images/hvm-ssd*/ubuntu-*-amd64-server-*")
                .build(),
        )
        .filters(Filter::builder().name("state").values("available").build())
        .filters(Filter::builder().name("architecture").values("x86_64").build())
        .filters(Filter::builder().name("root-device-type").values("ebs").build())
        .send()
        .await
        .map_err(fmt_err)?;
    let mut images = imgs.images().to_vec();
    images.sort_by(|a, b| {
        b.creation_date()
            .unwrap_or_default()
            .cmp(a.creation_date().unwrap_or_default())
    });
    let ami = images
        .first()
        .and_then(|i| i.image_id())
        .ok_or_else(|| "No Ubuntu AMI found in this region.".to_string())?
        .to_string();

    // 2. Key pair — save the private key locally for SSH.
    let kp = client
        .create_key_pair()
        .key_name(&name)
        .send()
        .await
        .map_err(fmt_err)?;
    let material = kp
        .key_material()
        .ok_or_else(|| "AWS returned no key material.".to_string())?
        .to_string();
    std::fs::create_dir_all(key_dir).map_err(|e| format!("create key dir: {e}"))?;
    let key_path = format!("{}/{}.pem", key_dir.trim_end_matches('/'), name);
    std::fs::write(&key_path, material).map_err(|e| format!("write key file: {e}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&key_path, std::fs::Permissions::from_mode(0o600));
    }

    // 3. Security group — SSH (22) from the caller's IP only.
    let my_ip = get_public_ip().await?;
    let sg = client
        .create_security_group()
        .group_name(&name)
        .description("AutoPipe managed SSH access")
        .send()
        .await
        .map_err(fmt_err)?;
    let sg_id = sg
        .group_id()
        .ok_or_else(|| "AWS returned no security group id.".to_string())?
        .to_string();
    client
        .authorize_security_group_ingress()
        .group_id(&sg_id)
        .ip_permissions(
            IpPermission::builder()
                .ip_protocol("tcp")
                .from_port(22)
                .to_port(22)
                .ip_ranges(
                    IpRange::builder()
                        .cidr_ip(format!("{my_ip}/32"))
                        .description("AutoPipe")
                        .build(),
                )
                .build(),
        )
        .send()
        .await
        .map_err(fmt_err)?;

    // 4. Launch with the install user-data (base64-encoded per the EC2 API).
    let user_data = base64::engine::general_purpose::STANDARD.encode(USER_DATA);
    let run = client
        .run_instances()
        .image_id(&ami)
        .instance_type(InstanceType::from(instance_type))
        .min_count(1)
        .max_count(1)
        .key_name(&name)
        .security_group_ids(&sg_id)
        .user_data(user_data)
        .iam_instance_profile(
            IamInstanceProfileSpecification::builder()
                .name(INSTANCE_PROFILE_NAME)
                .build(),
        )
        .tag_specifications(
            TagSpecification::builder()
                .resource_type(ResourceType::Instance)
                .tags(Tag::builder().key("autopipe").value("true").build())
                .tags(Tag::builder().key("Name").value(&name).build())
                .build(),
        )
        .send()
        .await
        .map_err(fmt_err)?;
    let instance_id = run
        .instances()
        .first()
        .and_then(|i| i.instance_id())
        .ok_or_else(|| "AWS returned no instance id.".to_string())?
        .to_string();

    // 5. Wait until running with a public IP (~ up to 5 min).
    let mut public_ip = String::new();
    for _ in 0..30 {
        tokio::time::sleep(Duration::from_secs(10)).await;
        let d = client
            .describe_instances()
            .instance_ids(instance_id.clone())
            .send()
            .await
            .map_err(fmt_err)?;
        if let Some(inst) = d
            .reservations()
            .first()
            .and_then(|r| r.instances().first())
        {
            let running =
                inst.state().and_then(|s| s.name()) == Some(&InstanceStateName::Running);
            if running {
                if let Some(ip) = inst.public_ip_address() {
                    public_ip = ip.to_string();
                    break;
                }
            }
        }
    }
    if public_ip.is_empty() {
        return Err(
            "The VM was created but didn't become reachable in time. Check the EC2 console."
                .into(),
        );
    }

    Ok(ProvisionResult {
        instance_id,
        sg_id,
        key_name: name,
        key_path,
        public_ip,
    })
}

/// Terminate the managed VM and best-effort clean up its security group + key.
pub async fn terminate_vm(
    access_key: &str,
    secret_key: &str,
    region: &str,
    instance_id: &str,
    sg_id: &str,
    key_name: &str,
) -> Result<(), String> {
    use aws_sdk_ec2::types::InstanceStateName;

    ensure_crypto();
    let client = ec2_client(access_key, secret_key, region);
    client
        .terminate_instances()
        .instance_ids(instance_id)
        .send()
        .await
        .map_err(fmt_err)?;

    // Wait until terminated so the security group can be deleted (capped ~2min).
    for _ in 0..24 {
        tokio::time::sleep(Duration::from_secs(5)).await;
        if let Ok(d) = client
            .describe_instances()
            .instance_ids(instance_id)
            .send()
            .await
        {
            let terminated = d
                .reservations()
                .first()
                .and_then(|r| r.instances().first())
                .and_then(|i| i.state())
                .and_then(|s| s.name())
                == Some(&InstanceStateName::Terminated);
            if terminated {
                break;
            }
        }
    }

    if !sg_id.is_empty() {
        let _ = client.delete_security_group().group_id(sg_id).send().await;
    }
    if !key_name.is_empty() {
        let _ = client.delete_key_pair().key_name(key_name).send().await;
    }
    Ok(())
}

/// Stop (halt) the managed VM. The EBS root disk and everything installed on it
/// persist; only that storage is billed while stopped (no compute charge).
pub async fn stop_vm(
    access_key: &str,
    secret_key: &str,
    region: &str,
    instance_id: &str,
) -> Result<(), String> {
    ensure_crypto();
    let client = ec2_client(access_key, secret_key, region);
    client
        .stop_instances()
        .instance_ids(instance_id)
        .send()
        .await
        .map_err(fmt_err)?;
    Ok(())
}

/// Start a previously stopped VM and wait for it to be running again. The public
/// IP changes across a stop/start cycle (there is no Elastic IP), so the new IP
/// is returned and must replace aws_vm_host.
pub async fn start_vm(
    access_key: &str,
    secret_key: &str,
    region: &str,
    instance_id: &str,
) -> Result<String, String> {
    use aws_sdk_ec2::types::InstanceStateName;

    ensure_crypto();
    let client = ec2_client(access_key, secret_key, region);
    client
        .start_instances()
        .instance_ids(instance_id)
        .send()
        .await
        .map_err(fmt_err)?;

    let mut public_ip = String::new();
    for _ in 0..30 {
        tokio::time::sleep(Duration::from_secs(10)).await;
        let d = client
            .describe_instances()
            .instance_ids(instance_id)
            .send()
            .await
            .map_err(fmt_err)?;
        if let Some(inst) = d
            .reservations()
            .first()
            .and_then(|r| r.instances().first())
        {
            let running =
                inst.state().and_then(|s| s.name()) == Some(&InstanceStateName::Running);
            if running {
                if let Some(ip) = inst.public_ip_address() {
                    public_ip = ip.to_string();
                    break;
                }
            }
        }
    }
    if public_ip.is_empty() {
        return Err(
            "The VM was started but didn't become reachable in time. Check the EC2 console.".into(),
        );
    }
    Ok(public_ip)
}

/// Current lifecycle state of the managed VM: "running", "stopped", "pending",
/// "stopping", "shutting-down", "terminated", or "unknown".
pub async fn vm_state(
    access_key: &str,
    secret_key: &str,
    region: &str,
    instance_id: &str,
) -> Result<String, String> {
    ensure_crypto();
    let client = ec2_client(access_key, secret_key, region);
    let d = client
        .describe_instances()
        .instance_ids(instance_id)
        .send()
        .await
        .map_err(fmt_err)?;
    let state = d
        .reservations()
        .first()
        .and_then(|r| r.instances().first())
        .and_then(|i| i.state())
        .and_then(|s| s.name())
        .map(|n| n.as_str().to_string())
        .unwrap_or_else(|| "unknown".to_string());
    Ok(state)
}
