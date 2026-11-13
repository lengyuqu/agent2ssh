//! macOS local discovery socket. It answers one keyless hello and nothing
//! else: it never reads the admin token and never stops the daemon. Stopping
//! is the Mach control channel's job (`daemon_identity::macos_mach`); two
//! local protocols that can both stop the process are not allowed to coexist.
use agent2ssh::daemon_identity::{macos::Instance, MAX_FRAME, VERSION};
use serde::{Deserialize, Serialize};
use std::{io, sync::Arc};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{UnixListener, UnixStream},
    sync::Semaphore,
    task::JoinSet,
    time::{timeout, Duration},
};

// `authorize` and `stop` are not variants on purpose: an unknown tag fails to
// parse, so a frame carrying a token is wiped without being interpreted.
#[derive(Deserialize)]
#[serde(tag = "type", deny_unknown_fields)]
enum Request {
    #[serde(rename = "hello")]
    Hello { version: u32, client_nonce: String },
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
    // A client may still send a token-bearing frame this server rejects.
    use zeroize::Zeroize;
    bytes.zeroize();
    result
}
async fn write(stream: &mut UnixStream, response: &Response<'_>) -> io::Result<()> {
    let bytes = serde_json::to_vec(response).map_err(|_| invalid())?;
    stream.write_u32(bytes.len() as u32).await?;
    stream.write_all(&bytes).await
}
async fn handle(mut stream: UnixStream, instance: Instance) -> io::Result<()> {
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
    // The client revalidates on this connection and expects it to stay open
    // and silent, so hold it until the peer closes or the caller's deadline.
    // Anything the peer sends now is discarded and ends the connection.
    let mut rest = [0u8; 1];
    let _ = stream.read(&mut rest).await;
    Ok(())
}

pub async fn serve(listener: UnixListener, instance: Instance) {
    let limit = Arc::new(Semaphore::new(16));
    let mut tasks = JoinSet::new();
    loop {
        tokio::select! {
            Some(_) = tasks.join_next(), if !tasks.is_empty() => {},
            result = listener.accept() => {
                let Ok((stream, _)) = result else { break };
                let Ok(permit) = limit.clone().try_acquire_owned() else { continue };
                let instance = instance.clone();
                tasks.spawn(async move {
                    let _permit = permit;
                    let _ = timeout(Duration::from_secs(5), handle(stream, instance)).await;
                });
            }
        }
    }
}
