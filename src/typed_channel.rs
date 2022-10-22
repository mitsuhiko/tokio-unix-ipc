use std::fmt;
use std::io;
use std::os::unix::io::{AsRawFd, FromRawFd, IntoRawFd, RawFd};
use std::os::unix::net::UnixStream;
use std::path::Path;

use serde_::de::DeserializeOwned;
use serde_::Serialize;

use crate::raw_channel::{raw_channel, RawReceiver, RawSender};
use crate::serde::{deserialize, serialize};

/// A typed receiver.
pub struct Receiver<T> {
    raw_receiver: RawReceiver,
    _marker: std::marker::PhantomData<T>,
}

/// A typed sender.
pub struct Sender<T> {
    raw_sender: RawSender,
    _marker: std::marker::PhantomData<T>,
}

impl<T> fmt::Debug for Receiver<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Receiver")
            .field("fd", &self.as_raw_fd())
            .finish()
    }
}

impl<T> fmt::Debug for Sender<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Sender")
            .field("fd", &self.as_raw_fd())
            .finish()
    }
}

macro_rules! fd_impl {
    ($field:ident, $raw_ty:ident, $ty:ty) => {
        #[allow(dead_code)]
        impl<T> $ty {
            pub(crate) unsafe fn from_raw_fd(fd: RawFd) -> io::Result<Self> {
                Ok(Self {
                    $field: $raw_ty::from_raw_fd(fd)?,
                    _marker: std::marker::PhantomData,
                })
            }

            pub(crate) fn from_std(stream: UnixStream) -> io::Result<Self> {
                Ok(Self {
                    $field: $raw_ty::from_std(stream)?,
                    _marker: std::marker::PhantomData,
                })
            }

            pub(crate) fn extract_raw_fd(&self) -> RawFd {
                self.$field.extract_raw_fd()
            }
        }

        impl<T: Serialize + DeserializeOwned> FromRawFd for $ty {
            unsafe fn from_raw_fd(fd: RawFd) -> Self {
                Self {
                    $field: FromRawFd::from_raw_fd(fd),
                    _marker: std::marker::PhantomData,
                }
            }
        }

        impl<T> IntoRawFd for $ty {
            fn into_raw_fd(self) -> RawFd {
                self.$field.into_raw_fd()
            }
        }

        impl<T: Serialize + DeserializeOwned> From<$raw_ty> for $ty {
            fn from(value: $raw_ty) -> Self {
                Self {
                    $field: value,
                    _marker: std::marker::PhantomData,
                }
            }
        }

        impl<T> AsRawFd for $ty {
            fn as_raw_fd(&self) -> RawFd {
                self.$field.as_raw_fd()
            }
        }
    };
}

fd_impl!(raw_receiver, RawReceiver, Receiver<T>);
fd_impl!(raw_sender, RawSender, Sender<T>);

/// Creates a typed connected channel.
pub fn channel<T: Serialize + DeserializeOwned>() -> io::Result<(Sender<T>, Receiver<T>)> {
    let (sender, receiver) = raw_channel()?;
    Ok((sender.into(), receiver.into()))
}

/// Creates a typed connected channel from an already extant socket.
pub fn channel_from_std<S: Serialize + DeserializeOwned, R: Serialize + DeserializeOwned>(
    sender: UnixStream,
) -> io::Result<(Sender<S>, Receiver<R>)> {
    let receiver = sender.try_clone()?;
    let sender = RawSender::from_std(sender)?;
    let receiver = RawReceiver::from_std(receiver)?;
    Ok((sender.into(), receiver.into()))
}

impl<T: Serialize + DeserializeOwned> Receiver<T> {
    /// Connects a receiver to a named unix socket.
    pub async fn connect<P: AsRef<Path>>(p: P) -> io::Result<Receiver<T>> {
        RawReceiver::connect(p).await.map(Into::into)
    }

    /// Converts the typed receiver into a raw one.
    pub fn into_raw_receiver(self) -> RawReceiver {
        self.raw_receiver
    }

    /// Receives a structured message from the socket.
    pub async fn recv(&self) -> io::Result<T> {
        let (buf, fds) = self.raw_receiver.recv().await?;
        deserialize::<(T, bool)>(&buf, fds.as_deref().unwrap_or_default()).map(|x| x.0)
    }
}

unsafe impl<T> Send for Receiver<T> {}
unsafe impl<T> Sync for Receiver<T> {}

impl<T: Serialize + DeserializeOwned> Sender<T> {
    /// Converts the typed sender into a raw one.
    pub fn into_raw_sender(self) -> RawSender {
        self.raw_sender
    }

    /// Receives a structured message from the socket.
    pub async fn send(&self, s: T) -> io::Result<()> {
        // we always serialize a dummy bool at the end so that the message
        // will not be empty because of zero sized types.
        let (payload, fds) = serialize((s, true))?;
        self.raw_sender.send(&payload, &fds).await?;
        Ok(())
    }
}

unsafe impl<T> Send for Sender<T> {}
unsafe impl<T> Sync for Sender<T> {}
