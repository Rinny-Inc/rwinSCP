use std::sync::mpsc::{Receiver, Sender};

use aws_sdk_s3::Client;
use aws_sdk_s3::config::{Credentials, Region};
use aws_sdk_s3::primitives::ByteStream;

use super::{Command, Event, RemoteEntry};
use crate::connection::{Auth, ConnectionProfile};

pub fn run(profile: ConnectionProfile, cmd_rx: Receiver<Command>, evt_tx: Sender<Event>) {
    let rt = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            evt_tx.send(Event::ConnectFailed(e.to_string())).ok();
            return;
        }
    };

    rt.block_on(async {
        let client = match build_client(&profile).await {
            Ok(c) => c,
            Err(e) => {
                evt_tx.send(Event::ConnectFailed(e.to_string())).ok();
                return;
            }
        };
        evt_tx.send(Event::Connected).ok();

        while let Ok(cmd) = cmd_rx.recv() {
            if let Err(e) = handle(&client, &profile, &cmd, &evt_tx).await {
                evt_tx.send(Event::Error(e.to_string())).ok();
            }
            if matches!(cmd, Command::Disconnect) {
                break;
            }
        }
        evt_tx.send(Event::Disconnected).ok();
    });
}

async fn build_client(profile: &ConnectionProfile) -> anyhow::Result<Client> {
    let (access_key, secret_key) = match &profile.auth {
        Auth::S3Keys {
            access_key,
            secret_key,
        } => (access_key.clone(), secret_key.clone()),
        _ => anyhow::bail!("S3 needs an access key / secret key pair"),
    };

    let creds = Credentials::new(access_key, secret_key, None, None, "rwinscp");
    let config = aws_config::defaults(aws_config::BehaviorVersion::latest())
        .region(Region::new(profile.region.clone()))
        .credentials_provider(creds)
        .load()
        .await;

    Ok(Client::new(&config))
}

async fn handle(
    client: &Client,
    profile: &ConnectionProfile,
    cmd: &Command,
    evt_tx: &Sender<Event>,
) -> anyhow::Result<()> {
    match cmd {
        Command::List { path } => {
            let prefix = path.trim_start_matches('/');
            let resp = client
                .list_objects_v2()
                .bucket(&profile.bucket)
                .delimiter("/")
                .prefix(prefix)
                .send()
                .await?;

            let mut entries = Vec::new();
            for p in resp.common_prefixes() {
                if let Some(prefix) = p.prefix() {
                    let name = prefix
                        .trim_end_matches('/')
                        .rsplit('/')
                        .next()
                        .unwrap_or(prefix);
                    entries.push(RemoteEntry {
                        name: name.to_string(),
                        is_dir: true,
                        size: 0,
                        modified: None,
                    });
                }
            }
            for obj in resp.contents() {
                if let Some(key) = obj.key() {
                    let name = key.rsplit('/').next().unwrap_or(key);
                    if name.is_empty() {
                        continue;
                    }
                    entries.push(RemoteEntry {
                        name: name.to_string(),
                        is_dir: false,
                        size: obj.size().unwrap_or(0) as u64,
                        modified: obj.last_modified().map(|d| d.to_string()),
                    });
                }
            }
            evt_tx
                .send(Event::Listing {
                    path: path.clone(),
                    entries,
                })
                .ok();
        }

        Command::Download {
            remote_path,
            local_path,
        } => {
            let key = remote_path.trim_start_matches('/');
            let mut obj = client
                .get_object()
                .bucket(&profile.bucket)
                .key(key)
                .send()
                .await?;
            let mut file = tokio::fs::File::create(local_path).await?;
            let mut transferred: u64 = 0;
            let total = obj.content_length().unwrap_or(0) as u64;
            use tokio::io::AsyncWriteExt;
            while let Some(chunk) = obj.body.try_next().await? {
                file.write_all(&chunk).await?;
                transferred += chunk.len() as u64;
                evt_tx
                    .send(Event::Progress {
                        transferred,
                        total,
                        label: remote_path.clone(),
                    })
                    .ok();
            }
            evt_tx
                .send(Event::TransferDone {
                    label: remote_path.clone(),
                })
                .ok();
        }

        Command::Upload {
            local_path,
            remote_path,
        } => {
            let key = remote_path.trim_start_matches('/');
            let body = ByteStream::from_path(local_path).await?;
            client
                .put_object()
                .bucket(&profile.bucket)
                .key(key)
                .body(body)
                .send()
                .await?;
            evt_tx
                .send(Event::TransferDone {
                    label: remote_path.clone(),
                })
                .ok();
        }

        Command::Mkdir { path } => {
            let key = format!("{}/", path.trim_start_matches('/').trim_end_matches('/'));
            client
                .put_object()
                .bucket(&profile.bucket)
                .key(key)
                .send()
                .await?;
        }

        Command::Delete { path, is_dir } => {
            let key = path.trim_start_matches('/');
            if *is_dir {
                let prefix = format!("{}/", key.trim_end_matches('/'));
                let resp = client
                    .list_objects_v2()
                    .bucket(&profile.bucket)
                    .prefix(&prefix)
                    .send()
                    .await?;
                for obj in resp.contents() {
                    if let Some(k) = obj.key() {
                        client
                            .delete_object()
                            .bucket(&profile.bucket)
                            .key(k)
                            .send()
                            .await?;
                    }
                }
            } else {
                client
                    .delete_object()
                    .bucket(&profile.bucket)
                    .key(key)
                    .send()
                    .await?;
            }
        }

        Command::Rename { from, to } => {
            let src_key = from.trim_start_matches('/');
            let dst_key = to.trim_start_matches('/');
            let copy_source = format!("{}/{}", profile.bucket, src_key);
            client
                .copy_object()
                .bucket(&profile.bucket)
                .copy_source(copy_source)
                .key(dst_key)
                .send()
                .await?;
            client
                .delete_object()
                .bucket(&profile.bucket)
                .key(src_key)
                .send()
                .await?;
        }

        Command::Exec { .. } => anyhow::bail!("S3 has no remote shell"),

        Command::Disconnect => {}
    }
    Ok(())
}
