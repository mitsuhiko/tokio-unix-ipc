use std::fs;
use std::io;
use std::os::unix::prelude::RawFd;
use std::path::{Path, PathBuf};
use tokio::sync::Mutex;

use tokio::net::UnixListener;

use crate::raw_channel::RawSender;

/// A bootstrap helper.
///
/// This creates a unix socket that is linked to the file system so
/// that a [`Receiver`](struct.Receiver.html) can connect to it.  It
/// lets you send one or more messages to the connected receiver.
///
/// The bootstrapper lets you send both to raw and typed receivers
/// on the other side. To send to a raw one use the
/// [`send_raw`](Self::send_raw) method.
#[derive(Debug)]
pub struct Bootstrapper {
    listener: UnixListener,
    sender: Mutex<Option<RawSender>>,
    path: PathBuf,
}

impl Bootstrapper {
    /// Creates a bootstrapper at a random socket in `/tmp`.
    pub fn new() -> io::Result<Bootstrapper> {
        use rand::{thread_rng, RngCore};
        use std::time::{SystemTime, UNIX_EPOCH};

        let mut dir = std::env::temp_dir();
        let mut rng = thread_rng();
        let now = SystemTime::now();
        dir.push(&format!(
            ".rust-unix-ipc.{}-{}.sock",
            now.duration_since(UNIX_EPOCH).unwrap().as_secs(),
            rng.next_u64(),
        ));
        Bootstrapper::bind(&dir)
    }

    /// Creates a bootstrapper at a specific socket path.
    pub fn bind<P: AsRef<Path>>(p: P) -> io::Result<Bootstrapper> {
        fs::remove_file(&p).ok();
        let listener = UnixListener::bind(&p)?;
        Ok(Bootstrapper {
            listener,
            sender: Mutex::new(None),
            path: p.as_ref().to_path_buf(),
        })
    }

    /// Returns the path of the socket.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Sends a raw value into the boostrapper.
    ///
    /// This can be called multiple times to send more than one value
    /// into the inner socket. On the other side a
    /// [`RawReceiver`](crate::RawReceiver) must be used.
    pub async fn send_raw(&self, data: &[u8], fds: &[RawFd]) -> io::Result<usize> {
        if self.sender.lock().await.is_none() {
            let (sock, _) = self.listener.accept().await?;
            let sender = RawSender::from_std(sock.into_std()?)?;
            *self.sender.lock().await = Some(sender);
        }
        let guard = self.sender.lock().await;

        guard.as_ref().unwrap().send(data, fds).await
    }

    /// Sends a value into the boostrapper.
    ///
    /// This can be called multiple times to send more than one value
    /// into the inner socket.  On the other side a correctly typed
    /// [`Receiver`](crate::Receiver) must be used.
    ///
    /// This requires the `serde` feature.
    #[cfg(feature = "serde")]
    pub async fn send<T: serde_::Serialize + serde_::de::DeserializeOwned>(
        &self,
        data: T,
    ) -> io::Result<()> {
        // replicate the logic from the typed sender with the dummy
        // bool here.
        let (bytes, fds) = crate::serde::serialize((data, true))?;
        self.send_raw(&bytes, &fds).await.map(|_| ())
    }
}

impl Drop for Bootstrapper {
    fn drop(&mut self) {
        fs::remove_file(&self.path).ok();
    }
}
