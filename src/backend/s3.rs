use std::sync::mpsc::{Receiver, Sender};

use aws_sdk_s3::Client;
use aws_sdk_s3::config::{Credentials, Region};
use aws_sdk_s3::primitives::ByteStream;
use tokio::io::AsyncWriteExt;

use super::{Command, Event, RemoteEntry};
use crate::backend::{Cancel, cancelled};
use crate::connection::{Auth, ConnectionProfile};

pub fn run(
    profile: ConnectionProfile,
    cmd_rx: Receiver<Command>,
    evt_tx: Sender<Event>,
    cancel: Cancel,
) {
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(e) => {
            evt_tx.send(Event::ConnectFailed(e.to_string())).ok();
            return;
        }
    };

    runtime.block_on(async move {
        let client = match build_client(&profile).await {
            Ok(client) => client,
            Err(e) => {
                evt_tx.send(Event::ConnectFailed(e.to_string())).ok();
                return;
            }
        };

        if let Err(e) = client
            .list_objects_v2()
            .bucket(&profile.bucket)
            .max_keys(1)
            .send()
            .await
        {
            evt_tx.send(Event::ConnectFailed(e.to_string())).ok();
            return;
        }
        evt_tx.send(Event::Connected).ok();

        while let Ok(cmd) = cmd_rx.recv() {
            let stop = matches!(cmd, Command::Disconnect);
            cancel.store(false, std::sync::atomic::Ordering::Relaxed);
            if let Err(e) = handle(&client, &profile, &cmd, &evt_tx, &cancel).await {
                evt_tx.send(Event::Error(e.to_string())).ok();
            }
            if stop {
                break;
            }
        }
        evt_tx.send(Event::Disconnected).ok();
    });
}

async fn build_client(profile: &ConnectionProfile) -> anyhow::Result<Client> {
    let Auth::S3Keys {
        access_key,
        secret_key,
    } = &profile.auth
    else {
        anyhow::bail!("S3 requires an access key / secret key pair");
    };

    let credentials = Credentials::new(
        access_key.clone(),
        secret_key.clone(),
        None,
        None,
        "rwinscp",
    );
    let config = aws_config::defaults(aws_config::BehaviorVersion::latest())
        .region(Region::new(profile.region.clone()))
        .credentials_provider(credentials)
        .load()
        .await;

    Ok(Client::new(&config))
}

async fn handle(
    client: &Client,
    profile: &ConnectionProfile,
    cmd: &Command,
    evt_tx: &Sender<Event>,
    cancel: &Cancel,
) -> anyhow::Result<()> {
    let bucket = profile.bucket.as_str();

    match cmd {
        Command::List { path } => {
            let prefix = as_prefix(path);
            let mut entries = Vec::new();
            let mut continuation: Option<String> = None;

            loop {
                let mut request = client
                    .list_objects_v2()
                    .bucket(bucket)
                    .delimiter("/")
                    .prefix(&prefix);
                if let Some(token) = &continuation {
                    request = request.continuation_token(token);
                }
                let response = request.send().await?;

                for common in response.common_prefixes() {
                    if let Some(full) = common.prefix() {
                        let name = full
                            .trim_end_matches('/')
                            .rsplit('/')
                            .next()
                            .unwrap_or(full);
                        if !name.is_empty() {
                            entries.push(RemoteEntry {
                                name: name.to_owned(),
                                is_dir: true,
                                size: 0,
                                modified: None,
                            });
                        }
                    }
                }

                for object in response.contents() {
                    let Some(key) = object.key() else { continue };
                    let name = key.rsplit('/').next().unwrap_or(key);
                    if name.is_empty() {
                        continue;
                    }
                    entries.push(RemoteEntry {
                        name: name.to_owned(),
                        is_dir: false,
                        size: object.size().unwrap_or(0).max(0) as u64,
                        modified: object.last_modified().and_then(format_timestamp),
                    });
                }

                match response.next_continuation_token() {
                    Some(token) if response.is_truncated().unwrap_or(false) => {
                        continuation = Some(token.to_owned());
                    }
                    _ => break,
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
        } if is_prefix(client, profile, remote_path).await => {
            download_tree(client, profile, remote_path, local_path, evt_tx, cancel).await?;
            evt_tx
                .send(Event::TransferDone {
                    label: remote_path.clone(),
                })
                .ok();
        }
        Command::Download {
            remote_path,
            local_path,
        } => {
            let key = as_key(remote_path);
            let mut object = client.get_object().bucket(bucket).key(&key).send().await?;
            let total = object.content_length().unwrap_or(0).max(0) as u64;

            let mut file = tokio::fs::File::create(local_path).await?;
            let mut transferred = 0u64;
            while let Some(chunk) = object.body.try_next().await? {
                if cancelled(cancel) {
                    anyhow::bail!("cancelled");
                }
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
            file.flush().await?;
            evt_tx
                .send(Event::TransferDone {
                    label: remote_path.clone(),
                })
                .ok();
        }

        Command::Upload {
            local_path,
            remote_path,
        } if local_path.is_dir() => {
            upload_tree(client, profile, local_path, remote_path, evt_tx, cancel).await?;
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
            let key = as_key(remote_path);
            let body = ByteStream::from_path(local_path).await?;
            client
                .put_object()
                .bucket(bucket)
                .key(&key)
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
            client
                .put_object()
                .bucket(bucket)
                .key(as_prefix(path))
                .send()
                .await?;
        }

        Command::Delete { path, is_dir } => {
            if *is_dir {
                let prefix = as_prefix(path);
                let mut continuation: Option<String> = None;
                loop {
                    let mut request = client.list_objects_v2().bucket(bucket).prefix(&prefix);
                    if let Some(token) = &continuation {
                        request = request.continuation_token(token);
                    }
                    let response = request.send().await?;

                    for object in response.contents() {
                        if let Some(key) = object.key() {
                            client
                                .delete_object()
                                .bucket(bucket)
                                .key(key)
                                .send()
                                .await?;
                        }
                    }

                    match response.next_continuation_token() {
                        Some(token) if response.is_truncated().unwrap_or(false) => {
                            continuation = Some(token.to_owned());
                        }
                        _ => break,
                    }
                }
            } else {
                client
                    .delete_object()
                    .bucket(bucket)
                    .key(as_key(path))
                    .send()
                    .await?;
            }
        }

        Command::Rename { from, to } => {
            // S3 has no rename: copy, then delete the source.
            let source = as_key(from);
            client
                .copy_object()
                .bucket(bucket)
                .copy_source(format!("{bucket}/{source}"))
                .key(as_key(to))
                .send()
                .await?;
            client
                .delete_object()
                .bucket(bucket)
                .key(&source)
                .send()
                .await?;
        }

        Command::ShellInput(_) => anyhow::bail!("S3 has not remote shell"),

        Command::Exec { .. } => anyhow::bail!("S3 has no remote shell"),

        Command::TrustHostKey => {}
        Command::Disconnect => {}
    }
    Ok(())
}

fn as_key(path: &str) -> String {
    path.trim_start_matches('/').to_owned()
}

fn as_prefix(path: &str) -> String {
    let trimmed = path.trim_matches('/');
    if trimmed.is_empty() {
        String::new()
    } else {
        format!("{trimmed}/")
    }
}

async fn is_prefix(client: &Client, profile: &ConnectionProfile, path: &str) -> bool {
    let key = as_key(path);
    if client
        .head_object()
        .bucket(&profile.bucket)
        .key(&key)
        .send()
        .await
        .is_ok()
    {
        return false;
    }
    let prefix = as_prefix(path);
    client
        .list_objects_v2()
        .bucket(&profile.bucket)
        .prefix(&prefix)
        .max_keys(1)
        .send()
        .await
        .map(|response| !response.contents().is_empty())
        .unwrap_or(false)
}

async fn upload_tree(
    client: &Client,
    profile: &ConnectionProfile,
    local_dir: &std::path::Path,
    remote_dir: &str,
    evt_tx: &Sender<Event>,
    cancel: &Cancel,
) -> anyhow::Result<()> {
    for entry in std::fs::read_dir(local_dir)? {
        if cancelled(cancel) {
            anyhow::bail!("cancelled");
        }
        let entry = entry?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        let remote_child = format!("{}/{name}", remote_dir.trim_end_matches('/'));

        if entry.file_type()?.is_dir() {
            Box::pin(upload_tree(
                client,
                profile,
                &entry.path(),
                &remote_child,
                evt_tx,
                cancel,
            ))
            .await?;
        } else {
            let path = entry.path();
            let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            let body = ByteStream::from_path(&path).await?;
            client
                .put_object()
                .bucket(&profile.bucket)
                .key(as_key(&remote_child))
                .body(body)
                .send()
                .await?;
            evt_tx
                .send(Event::Progress {
                    transferred: size,
                    total: size,
                    label: remote_child.clone(),
                })
                .ok();
        }
    }
    Ok(())
}
async fn download_tree(
    client: &Client,
    profile: &ConnectionProfile,
    remote_dir: &str,
    local_dir: &std::path::Path,
    evt_tx: &Sender<Event>,
    cancel: &Cancel,
) -> anyhow::Result<()> {
    use tokio::io::AsyncWriteExt;

    let prefix = as_prefix(remote_dir);
    let mut continuation: Option<String> = None;
    std::fs::create_dir_all(local_dir)?;

    loop {
        let mut request = client
            .list_objects_v2()
            .bucket(&profile.bucket)
            .prefix(&prefix);
        if let Some(token) = &continuation {
            request = request.continuation_token(token);
        }
        let response = request.send().await?;

        for object in response.contents() {
            if cancelled(cancel) {
                anyhow::bail!("cancelled");
            }
            let Some(key) = object.key() else {
                continue;
            };
            if key.ends_with('/') {
                continue;
            }

            let relative = key.strip_prefix(&prefix).unwrap_or(key);
            let target = local_dir.join(relative);

            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let mut object = client
                .get_object()
                .bucket(&profile.bucket)
                .key(key)
                .send()
                .await?;
            let mut file = tokio::fs::File::create(&target).await?;
            let mut moved = 0u64;
            while let Some(chunk) = object.body.try_next().await? {
                if cancelled(cancel) {
                    anyhow::bail!("cancelled");
                }
                file.write_all(&chunk).await?;
                moved += chunk.len() as u64;
            }
            file.flush().await?;
            evt_tx
                .send(Event::Progress {
                    transferred: moved,
                    total: moved,
                    label: key.to_owned(),
                })
                .ok();
        }

        match response.next_continuation_token() {
            Some(token) if response.is_truncated().unwrap_or(false) => {
                continuation = Some(token.to_owned());
            }
            _ => break,
        }
    }
    Ok(())
}

fn format_timestamp(dt: &aws_sdk_s3::primitives::DateTime) -> Option<String> {
    chrono::DateTime::from_timestamp(dt.secs(), 0).map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
}
