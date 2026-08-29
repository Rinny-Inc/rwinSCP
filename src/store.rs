use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::app::Host;
use crate::connection::{Auth, ConnectionProfile, Protocol};

const SERVICE: &str = "rwinSCP";
const FILE_NAME: &str = "hosts.json";

#[derive(Serialize, Deserialize)]
struct StoredHost {
    id: String,
    name: String,
    protocol: String,
    host: String,
    port: u16,
    username: String,
    #[serde(default)]
    bucket: String,
    #[serde(default)]
    region: String,
    #[serde(default)]
    remote_start_dir: String,
    auth_kind: String,
    #[serde(default)]
    key_path: String,
    #[serde(default)]
    access_key: String,
}

pub fn config_path() -> Option<PathBuf> {
    dirs::config_dir().map(|dir| dir.join("rwinSCP").join(FILE_NAME))
}

pub fn load() -> Vec<Host> {
    let Some(path) = config_path() else {
        return Vec::new();
    };
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    let Ok(stored) = serde_json::from_str::<Vec<StoredHost>>(&text) else {
        return Vec::new();
    };

    stored.into_iter().map(into_host).collect()
}

pub fn save(hosts: &[Host]) -> anyhow::Result<()> {
    let Some(path) = config_path() else {
        anyhow::bail!("no config directory on this platform");
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let stored: Vec<StoredHost> = hosts.iter().map(from_host).collect();
    let text = serde_json::to_string_pretty(&stored)?;

    let temp = path.with_extension("json.tmp");
    std::fs::write(&temp, text)?;
    std::fs::rename(&temp, &path)?;

    Ok(())
}

pub fn save_secret(host: &Host) -> bool {
    let secret = match &host.profile.auth {
        Auth::Password(pswrd) => pswrd,
        Auth::KeyFile { passphrase, .. } => passphrase,
        Auth::S3Keys { secret_key, .. } => secret_key,
    };

    let Ok(entry) = keyring::Entry::new(SERVICE, &host.id) else {
        return false;
    };

    if secret.is_empty() {
        entry.delete_credential().ok();
        return true;
    }

    entry.set_password(secret).is_ok()
}

pub fn forget_secret(id: &str) {
    let Ok(entry) = keyring::Entry::new(SERVICE, id) else {
        return;
    };

    entry.delete_credential().ok();
}

fn load_secret(id: &str) -> String {
    keyring::Entry::new(SERVICE, id)
        .and_then(|entry| entry.get_password())
        .unwrap_or_default()
}

pub fn new_id() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("host-{nanos:x}")
}

fn protocol_name(protocol: Protocol) -> &'static str {
    match protocol {
        Protocol::Ssh => "ssh",
        Protocol::Sftp => "sftp",
        Protocol::Scp => "scp",
        Protocol::Ftp => "ftp",
        Protocol::S3 => "s3",
    }
}

fn protocol_from_name(name: &str) -> Protocol {
    match name {
        "ssh" => Protocol::Ssh,
        "scp" => Protocol::Scp,
        "ftp" => Protocol::Ftp,
        "s3" => Protocol::S3,
        _ => Protocol::Sftp,
    }
}

fn from_host(host: &Host) -> StoredHost {
    let profile = &host.profile;
    let (auth_kind, key_path, access_key) = match &profile.auth {
        Auth::Password(_) => ("password", String::new(), String::new()),
        Auth::KeyFile { path, .. } => ("key", path.clone(), String::new()),
        Auth::S3Keys { access_key, .. } => ("s3", String::new(), access_key.clone()),
    };

    StoredHost {
        id: host.id.clone(),
        name: profile.name.clone(),
        protocol: protocol_name(profile.protocol).to_owned(),
        host: profile.host.clone(),
        port: profile.port,
        username: profile.username.clone(),
        bucket: profile.bucket.clone(),
        region: profile.region.clone(),
        remote_start_dir: profile.remote_start_dir.clone(),
        auth_kind: auth_kind.to_owned(),
        key_path,
        access_key,
    }
}

fn into_host(stored: StoredHost) -> Host {
    let secret = load_secret(&stored.id);

    let auth = match stored.auth_kind.as_str() {
        "key" => Auth::KeyFile {
            path: stored.key_path,
            passphrase: secret,
        },
        "s3" => Auth::S3Keys {
            access_key: stored.access_key,
            secret_key: secret,
        },
        _ => Auth::Password(secret),
    };

    let protocol = protocol_from_name(&stored.protocol);
    let profile = ConnectionProfile {
        name: stored.name,
        protocol,
        host: stored.host,
        port: stored.port,
        username: stored.username,
        auth,
        bucket: stored.bucket,
        region: stored.region,
        remote_start_dir: if stored.remote_start_dir.is_empty() {
            "/".to_owned()
        } else {
            stored.remote_start_dir
        },
    };

    Host {
        id: stored.id,
        profile,
        last_used: None,
    }
}
