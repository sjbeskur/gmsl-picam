//! Serialized UDP command execution, with bounded retry deduplication.
use crate::control::{CameraService, ControlError};
use photon_protocol::{
    Command, CommandPacket, CommandResponse, CommandResult, ErrorCode, MAX_COMMAND_BYTES,
};
use std::{
    collections::VecDeque,
    io,
    net::SocketAddr,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::net::UdpSocket;

const CACHE_TTL: Duration = Duration::from_secs(30);
const CACHE_CAPACITY: usize = 256;
struct Cached {
    peer: SocketAddr,
    id: u64,
    request: Vec<u8>,
    response: Vec<u8>,
    at: Instant,
}

fn execute(service: &CameraService, command: Command) -> CommandResult {
    let result = match command {
        Command::Ping => return CommandResult::Pong,
        Command::Status => service
            .status()
            .map_err(|e| ControlError::Internal(e.to_string())),
        Command::Start => service
            .start()
            .map_err(|e| ControlError::Internal(e.to_string())),
        Command::Stop => service
            .stop()
            .map_err(|e| ControlError::Internal(e.to_string())),
        Command::Settings(patch) => service.update(patch),
    };
    match result {
        Ok(status) => CommandResult::Status(Box::new(status)),
        Err(error) => {
            let (code, message) = match error {
                ControlError::Invalid(m) => (ErrorCode::Invalid, m),
                ControlError::Conflict(m) => (ErrorCode::Conflict, m),
                ControlError::Internal(m) => (ErrorCode::Unavailable, m),
            };
            CommandResult::Error {
                code,
                message: message.chars().take(512).collect(),
            }
        }
    }
}

/// Each datagram contains one command. Duplicates return the cached reply, and
/// reusing an ID with different contents is rejected. This is a bounded retry
/// window, not persistent exactly-once delivery across daemon restarts.
pub async fn serve(socket: UdpSocket, service: Arc<CameraService>) -> io::Result<()> {
    let mut buffer = [0; MAX_COMMAND_BYTES + 1];
    let mut cache: VecDeque<Cached> = VecDeque::new();
    loop {
        let (len, peer) = socket.recv_from(&mut buffer).await?;
        let packet = match CommandPacket::decode(&buffer[..len]) {
            Ok(packet) => packet,
            Err(error) => {
                tracing::debug!(%peer, %error, "discarded invalid UDP command");
                continue;
            }
        };
        while cache
            .front()
            .is_some_and(|entry| entry.at.elapsed() > CACHE_TTL)
        {
            cache.pop_front();
        }
        let response = if let Some(entry) = cache
            .iter()
            .find(|e| e.peer == peer && e.id == packet.request_id)
        {
            if entry.request == buffer[..len] {
                entry.response.clone()
            } else {
                CommandResponse::new(
                    packet.request_id,
                    CommandResult::Error {
                        code: ErrorCode::Invalid,
                        message: "request ID reused with different contents".into(),
                    },
                )
                .encode()?
            }
        } else {
            let controller = Arc::clone(&service);
            let result = tokio::task::spawn_blocking(move || execute(&controller, packet.command))
                .await
                .map_err(io::Error::other)?;
            let response = CommandResponse::new(packet.request_id, result)
                .encode()
                .or_else(|_| {
                    CommandResponse::new(
                        packet.request_id,
                        CommandResult::Error {
                            code: ErrorCode::Unavailable,
                            message: "response exceeds protocol limit".into(),
                        },
                    )
                    .encode()
                })?;
            if cache.len() == CACHE_CAPACITY {
                cache.pop_front();
            }
            cache.push_back(Cached {
                peer,
                id: packet.request_id,
                request: buffer[..len].to_vec(),
                response: response.clone(),
                at: Instant::now(),
            });
            response
        };
        // A vanished UDP peer must not bring down the daemon.
        if let Err(error) = socket.send_to(&response, peer).await {
            tracing::debug!(%peer, %error, "UDP reply failed");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::camera::CameraConfig;
    use std::sync::atomic::{AtomicUsize, Ordering};
    #[tokio::test]
    async fn retries_are_deduplicated_and_bad_packets_do_not_stop_control() {
        let starts = Arc::new(AtomicUsize::new(0));
        let count = Arc::clone(&starts);
        let service = Arc::new(
            CameraService::with_capture(
                CameraConfig::default(),
                Arc::new(move |_, _, stop, _| {
                    count.fetch_add(1, Ordering::SeqCst);
                    while !stop.load(Ordering::Acquire) {
                        std::thread::sleep(Duration::from_millis(1));
                    }
                    Ok(())
                }),
            )
            .unwrap(),
        );
        let listener = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(serve(listener, Arc::clone(&service)));
        let client = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        client.connect(address).await.unwrap();
        client.send(b"invalid").await.unwrap();
        client.send(&[0; MAX_COMMAND_BYTES + 1]).await.unwrap();
        let start = CommandPacket::new(7, Command::Start).encode().unwrap();
        client.send(&start).await.unwrap();
        let mut buf = [0; 4096];
        let len = tokio::time::timeout(Duration::from_secs(2), client.recv(&mut buf))
            .await
            .unwrap()
            .unwrap();
        let first = buf[..len].to_vec();
        assert!(matches!(
            CommandResponse::decode(&first).unwrap().result,
            CommandResult::Status(_)
        ));
        service.stop().unwrap();
        assert_eq!(starts.load(Ordering::SeqCst), 1);
        client.send(&start).await.unwrap();
        let len = client.recv(&mut buf).await.unwrap();
        assert_eq!(&buf[..len], first);
        assert!(!service.status().unwrap().capture_active);
        assert_eq!(starts.load(Ordering::SeqCst), 1);
        client
            .send(&CommandPacket::new(7, Command::Stop).encode().unwrap())
            .await
            .unwrap();
        let len = client.recv(&mut buf).await.unwrap();
        assert!(matches!(
            CommandResponse::decode(&buf[..len]).unwrap().result,
            CommandResult::Error {
                code: ErrorCode::Invalid,
                ..
            }
        ));
        client
            .send(
                &CommandPacket::new(
                    8,
                    Command::Settings(photon_protocol::SettingsPatch {
                        gain: Some(0.0),
                        ..Default::default()
                    }),
                )
                .encode()
                .unwrap(),
            )
            .await
            .unwrap();
        let len = client.recv(&mut buf).await.unwrap();
        assert!(matches!(
            CommandResponse::decode(&buf[..len]).unwrap().result,
            CommandResult::Error {
                code: ErrorCode::Invalid,
                ..
            }
        ));
        server.abort();
        let _ = server.await;
    }
}
