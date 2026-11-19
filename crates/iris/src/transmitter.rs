//! Image output adapters. Network I/O runs off the capture thread; watch updates
//! coalesce while an output is slow or disconnected.
use crate::frame_store::FrameStore;
use photon_protocol::{write_postcard, ImagePostcard};
use std::{
    io::{self, Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    str::FromStr,
    sync::Arc,
    time::{Duration, Instant},
};
#[cfg(unix)]
use std::{
    os::unix::{
        fs::MetadataExt,
        net::{UnixListener, UnixStream},
    },
    path::PathBuf,
};
use tokio::sync::watch;

const IO_TIMEOUT: Duration = Duration::from_millis(500);

struct DeadlineWriter<'a, W> {
    inner: &'a mut W,
    deadline: Instant,
}
impl<W: Write> Write for DeadlineWriter<'_, W> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if Instant::now() >= self.deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "postcard write deadline",
            ));
        }
        self.inner.write(bytes)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

pub trait Transmitter: Send + 'static {
    /// May block only within the adapter's bounded connect/write timeout.
    fn transmit(&mut self, postcard: &ImagePostcard) -> io::Result<()>;
}

#[derive(Debug, Clone)]
pub enum StreamEndpoint {
    Tcp(SocketAddr),
    #[cfg(unix)]
    Unix(PathBuf),
}
impl FromStr for StreamEndpoint {
    type Err = io::Error;
    fn from_str(value: &str) -> io::Result<Self> {
        if let Some(address) = value.strip_prefix("tcp://") {
            return address
                .parse()
                .map(Self::Tcp)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e));
        }
        #[cfg(unix)]
        if let Some(path) = value.strip_prefix("unix://") {
            if path.starts_with('/') && path.len() > 1 {
                return Ok(Self::Unix(path.into()));
            }
        }
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "expected tcp://IP:port or unix:///absolute/path",
        ))
    }
}

pub enum ImageStream {
    Tcp(TcpStream),
    #[cfg(unix)]
    Unix(UnixStream),
}
impl ImageStream {
    pub fn set_read_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
        match self {
            Self::Tcp(s) => s.set_read_timeout(timeout),
            #[cfg(unix)]
            Self::Unix(s) => s.set_read_timeout(timeout),
        }
    }
}
impl Read for ImageStream {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        match self {
            Self::Tcp(s) => s.read(buffer),
            #[cfg(unix)]
            Self::Unix(s) => s.read(buffer),
        }
    }
}
impl Write for ImageStream {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        match self {
            Self::Tcp(s) => s.write(buffer),
            #[cfg(unix)]
            Self::Unix(s) => s.write(buffer),
        }
    }
    fn flush(&mut self) -> io::Result<()> {
        match self {
            Self::Tcp(s) => s.flush(),
            #[cfg(unix)]
            Self::Unix(s) => s.flush(),
        }
    }
}

pub struct TcpTransmitter {
    address: SocketAddr,
    stream: Option<TcpStream>,
}
impl TcpTransmitter {
    pub fn new(address: SocketAddr) -> Self {
        Self {
            address,
            stream: None,
        }
    }
}
impl Transmitter for TcpTransmitter {
    fn transmit(&mut self, postcard: &ImagePostcard) -> io::Result<()> {
        if self.stream.is_none() {
            let stream = TcpStream::connect_timeout(&self.address, IO_TIMEOUT)?;
            stream.set_write_timeout(Some(IO_TIMEOUT))?;
            stream.set_nodelay(true)?;
            self.stream = Some(stream);
        }
        let result = write_postcard(
            &mut DeadlineWriter {
                inner: self.stream.as_mut().unwrap(),
                deadline: Instant::now() + IO_TIMEOUT,
            },
            postcard,
        );
        if result.is_err() {
            self.stream = None;
        }
        result
    }
}

#[cfg(unix)]
pub struct UnixTransmitter {
    path: PathBuf,
    stream: Option<UnixStream>,
}
#[cfg(unix)]
impl UnixTransmitter {
    pub fn new(path: PathBuf) -> Self {
        Self { path, stream: None }
    }
}
#[cfg(unix)]
impl Transmitter for UnixTransmitter {
    fn transmit(&mut self, postcard: &ImagePostcard) -> io::Result<()> {
        if self.stream.is_none() {
            // Nonblocking connect bounds the time spent on a full listener queue.
            let socket = socket2::Socket::new(socket2::Domain::UNIX, socket2::Type::STREAM, None)?;
            socket.connect_timeout(&socket2::SockAddr::unix(&self.path)?, IO_TIMEOUT)?;
            let stream: UnixStream = socket.into();
            stream.set_write_timeout(Some(IO_TIMEOUT))?;
            self.stream = Some(stream);
        }
        let result = write_postcard(
            &mut DeadlineWriter {
                inner: self.stream.as_mut().unwrap(),
                deadline: Instant::now() + IO_TIMEOUT,
            },
            postcard,
        );
        if result.is_err() {
            self.stream = None;
        }
        result
    }
}

impl StreamEndpoint {
    pub fn transmitter(&self) -> Box<dyn Transmitter> {
        match self {
            Self::Tcp(a) => Box::new(TcpTransmitter::new(*a)),
            #[cfg(unix)]
            Self::Unix(p) => Box::new(UnixTransmitter::new(p.clone())),
        }
    }
}

pub enum PostcardListener {
    Tcp(TcpListener),
    #[cfg(unix)]
    Unix {
        listener: UnixListener,
        path: PathBuf,
        device: u64,
        inode: u64,
    },
}
impl PostcardListener {
    pub fn bind(endpoint: &StreamEndpoint) -> io::Result<Self> {
        match endpoint {
            StreamEndpoint::Tcp(a) => {
                let listener = TcpListener::bind(a)?;
                listener.set_nonblocking(true)?;
                Ok(Self::Tcp(listener))
            }
            #[cfg(unix)]
            StreamEndpoint::Unix(path) => {
                // Never unlink an existing socket: it may belong to a live process.
                let listener = UnixListener::bind(path)?;
                listener.set_nonblocking(true)?;
                let metadata = std::fs::symlink_metadata(path)?;
                Ok(Self::Unix {
                    listener,
                    path: path.clone(),
                    device: metadata.dev(),
                    inode: metadata.ino(),
                })
            }
        }
    }
    pub fn accept(&self) -> io::Result<ImageStream> {
        match self {
            Self::Tcp(l) => l.accept().map(|(s, _)| ImageStream::Tcp(s)),
            #[cfg(unix)]
            Self::Unix { listener, .. } => listener.accept().map(|(s, _)| ImageStream::Unix(s)),
        }
    }
}
impl Drop for PostcardListener {
    fn drop(&mut self) {
        #[cfg(unix)]
        if let Self::Unix {
            path,
            device,
            inode,
            ..
        } = self
        {
            if std::fs::symlink_metadata(&path)
                .is_ok_and(|m| m.dev() == *device && m.ino() == *inode)
            {
                let _ = std::fs::remove_file(path);
            }
        }
    }
}

pub async fn run(
    mut transmitter: Box<dyn Transmitter>,
    store: Arc<FrameStore>,
    mut shutdown: watch::Receiver<bool>,
) -> io::Result<()> {
    let mut changes = store.subscribe();
    let mut retry = tokio::time::interval(Duration::from_millis(250));
    retry.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut sent = None;
    let mut disconnected = false;
    loop {
        tokio::select! {
            _ = shutdown.changed() => break,
            _ = changes.changed() => {},
            _ = retry.tick() => {},
        }
        if *shutdown.borrow() {
            break;
        }
        let Some(frame) = store.fresh_frame() else {
            continue;
        };
        let id = (frame.metadata.sensor_ts_ns, frame.metadata.frame_index);
        if sent == Some(id) {
            continue;
        }
        let (returned, result) = tokio::task::spawn_blocking(move || {
            let result = transmitter.transmit(&frame);
            (transmitter, result)
        })
        .await
        .map_err(io::Error::other)?;
        transmitter = returned;
        match result {
            Ok(()) => {
                sent = Some(id);
                if disconnected {
                    tracing::info!("image transmitter connected");
                }
                disconnected = false;
            }
            Err(error) => {
                if !disconnected {
                    tracing::warn!(%error, "image transmitter disconnected; capture continues");
                }
                disconnected = true;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use photon_protocol::{CameraState, FrameMetadata};
    use std::sync::mpsc;
    fn metadata(index: u64) -> FrameMetadata {
        FrameMetadata {
            frame_index: index,
            sensor_ts_ns: index as i64 + 1,
            exposure_us: None,
            analogue_gain: None,
            coi_ns: None,
            first_row_coi_ns: None,
            frame_duration_us: None,
            width: 2,
            height: 2,
            pixel_format: "NV12".into(),
            captured_at_unix_ms: 0,
        }
    }
    struct Slow {
        seen: mpsc::Sender<u64>,
        release: mpsc::Receiver<()>,
    }
    impl Transmitter for Slow {
        fn transmit(&mut self, postcard: &ImagePostcard) -> io::Result<()> {
            self.seen.send(postcard.metadata.frame_index).unwrap();
            if postcard.metadata.frame_index == 1 {
                self.release.recv_timeout(Duration::from_secs(2)).unwrap();
            }
            Ok(())
        }
    }
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn slow_transmitter_skips_intermediate_frames_without_blocking_capture() {
        let frames = Arc::new(FrameStore::new(CameraState::Stopped, 2, 2, "NV12"));
        let (seen_tx, seen_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let (stop, shutdown) = watch::channel(false);
        frames.update_frame(vec![1], metadata(1));
        let task = tokio::spawn(run(
            Box::new(Slow {
                seen: seen_tx,
                release: release_rx,
            }),
            Arc::clone(&frames),
            shutdown,
        ));
        assert_eq!(seen_rx.recv_timeout(Duration::from_secs(2)).unwrap(), 1);
        for index in 2..=100 {
            frames.update_frame(vec![index as u8], metadata(index));
        }
        assert_eq!(frames.status().frame_count, 100);
        release_tx.send(()).unwrap();
        assert_eq!(seen_rx.recv_timeout(Duration::from_secs(2)).unwrap(), 100);
        stop.send_replace(true);
        tokio::time::timeout(Duration::from_secs(2), task)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
    }
}
