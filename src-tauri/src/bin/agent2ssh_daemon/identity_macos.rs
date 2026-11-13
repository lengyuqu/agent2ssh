//! macOS local IPC server. Client-side process binding is a separate condition;
//! receipt of an authorization here does not prove that the client has it.
use agent2ssh::daemon_identity::{macos::Instance, MAX_FRAME, VERSION};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{io, sync::Arc};
use subtle::ConstantTimeEq;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{UnixListener, UnixStream},
    sync::Semaphore,
    task::JoinSet,
    time::{timeout, Duration},
};

#[derive(Deserialize)]
#[serde(tag = "type", deny_unknown_fields)]
enum Request {
    #[serde(rename = "hello")]
    Hello { version: u32, client_nonce: String },
    #[serde(rename = "authorize")]
    Authorize { version: u32, admin_token: String },
    #[serde(rename = "stop")]
    Stop {
        version: u32,
        instance_id: String,
        expect_pid: u32,
        request_id: String,
    },
}
#[derive(Serialize)]
#[serde(tag = "type")]
enum Response<'a> {
    #[serde(rename = "hello_ok")]
    Hello {
        version: u32,
        client_nonce: &'a str,
        instance_id: &'a str,
        pid: u32,
    },
    #[serde(rename = "authorized")]
    Authorized { version: u32, result: &'static str },
    #[serde(rename = "stop_result")]
    Stop {
        version: u32,
        instance_id: &'a str,
        request_id: &'a str,
        result: &'static str,
        checked_pid: u32,
    },
}
fn id(s: &str) -> bool {
    s.len() == 32
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn invalid() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "invalid IPC request")
}
async fn read(stream: &mut UnixStream) -> io::Result<Request> {
    let n = stream.read_u32().await? as usize;
    if n == 0 || n > MAX_FRAME {
        return Err(invalid());
    }
    let mut bytes = vec![0; n];
    stream.read_exact(&mut bytes).await?;
    let result = serde_json::from_slice(&bytes).map_err(|_| invalid());
    // Minimize the lifetime of token-containing frames in the buffer.
    use zeroize::Zeroize;
    bytes.zeroize();
    result
}
async fn write(stream: &mut UnixStream, response: &Response<'_>) -> io::Result<()> {
    let bytes = serde_json::to_vec(response).map_err(|_| invalid())?;
    stream.write_u32(bytes.len() as u32).await?;
    stream.write_all(&bytes).await
}
async fn handle(mut stream: UnixStream, instance: Instance, token: Arc<String>) -> io::Result<()> {
    // Unix permissions alone are not authentication. Reject other UIDs even if
    // directory permissions are changed while the daemon is running.
    let cred = stream.peer_cred()?;
    if cred.uid() != unsafe { libc::geteuid() } {
        return Err(invalid());
    }
    let nonce = match read(&mut stream).await? {
        Request::Hello {
            version: VERSION,
            client_nonce,
        } if id(&client_nonce) => client_nonce,
        _ => return Err(invalid()),
    };
    write(
        &mut stream,
        &Response::Hello {
            version: VERSION,
            client_nonce: &nonce,
            instance_id: &instance.instance_id,
            pid: instance.pid,
        },
    )
    .await?;
    let supplied = match read(&mut stream).await? {
        Request::Authorize {
            version: VERSION,
            admin_token,
        } => zeroize::Zeroizing::new(admin_token),
        _ => return Err(invalid()),
    };
    // Fixed-size digests avoid a content-dependent comparison of the secrets.
    let supplied_hash = Sha256::digest(supplied.as_bytes());
    let expected_hash = Sha256::digest(token.as_bytes());
    if !bool::from(supplied_hash.ct_eq(&expected_hash)) {
        return Err(invalid());
    }
    drop(supplied);
    write(
        &mut stream,
        &Response::Authorized {
            version: VERSION,
            result: "ok",
        },
    )
    .await?;
    let request_id = match read(&mut stream).await? {
        Request::Stop {
            version: VERSION,
            instance_id,
            expect_pid,
            request_id,
        } if instance_id == instance.instance_id
            && expect_pid == instance.pid
            && id(&request_id) =>
        {
            request_id
        }
        _ => return Err(invalid()),
    };
    let sent = write(
        &mut stream,
        &Response::Stop {
            version: VERSION,
            instance_id: &instance.instance_id,
            request_id: &request_id,
            result: "accepted",
            checked_pid: instance.pid,
        },
    )
    .await;
    // A lost receipt is an unknown outcome. Never replay it automatically.
    super::IPC_SHUTDOWN_REQUESTED.notify_one();
    sent
}

pub async fn serve(listener: UnixListener, instance: Instance, token: String) {
    let token = Arc::new(token);
    let limit = Arc::new(Semaphore::new(16));
    let mut tasks = JoinSet::new();
    loop {
        tokio::select! {
            Some(_) = tasks.join_next(), if !tasks.is_empty() => {},
            result = listener.accept() => {
                let Ok((stream, _)) = result else { break };
                let Ok(permit) = limit.clone().try_acquire_owned() else { continue };
                let instance = instance.clone();
                let token = token.clone();
                tasks.spawn(async move {
                    let _permit = permit;
                    let _ = timeout(Duration::from_secs(5), handle(stream, instance, token)).await;
                });
            }
        }
    }
}
