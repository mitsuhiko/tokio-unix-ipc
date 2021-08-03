use std::io;
use std::mem;
use std::os::unix::io::{AsRawFd, FromRawFd, IntoRawFd, RawFd};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::slice;
use std::sync::atomic::{AtomicBool, Ordering};

use nix::errno::Errno;
use nix::sys::socket::{
    c_uint, recvmsg, sendmsg, ControlMessage, ControlMessageOwned, MsgFlags, CMSG_SPACE,
};
use nix::sys::uio::IoVec;
use nix::unistd;

use tokio::io::unix::AsyncFd;

#[cfg(target_os = "linux")]
const MSG_FLAGS: MsgFlags = MsgFlags::MSG_CMSG_CLOEXEC;

#[cfg(target_os = "macos")]
const MSG_FLAGS: MsgFlags = MsgFlags::empty();

#[repr(C)]
#[derive(Default, Debug)]
struct MsgHeader {
    payload_len: u32,
    fd_count: u32,
}

impl MsgHeader {
    pub fn as_buf(&self) -> &[u8] {
        unsafe { slice::from_raw_parts((self as *const _) as *const u8, mem::size_of_val(self)) }
    }

    pub fn as_buf_mut(&mut self) -> &mut [u8] {
        unsafe { slice::from_raw_parts_mut((self as *mut _) as *mut u8, mem::size_of_val(self)) }
    }

    pub fn make_buffer(&self) -> Vec<u8> {
        vec![0u8; self.payload_len as usize]
    }
}

macro_rules! fd_impl {
    ($ty:ty) => {
        #[allow(dead_code)]
        impl $ty {
            pub(crate) unsafe fn from_raw_fd(fd: RawFd) -> io::Result<Self> {
                Ok(Self {
                    inner: AsyncFd::new(fd)?,
                    dead: AtomicBool::new(false),
                })
            }

            pub(crate) fn from_std(stream: UnixStream) -> io::Result<Self> {
                unsafe { Self::from_raw_fd(stream.into_raw_fd()) }
            }

            pub(crate) fn extract_raw_fd(&self) -> RawFd {
                if self.dead.swap(true, Ordering::SeqCst) {
                    panic!("handle was moved previously");
                } else {
                    self.inner.as_raw_fd()
                }
            }
        }

        impl FromRawFd for $ty {
            unsafe fn from_raw_fd(fd: RawFd) -> Self {
                Self::from_raw_fd(fd)
                    .expect("conversion from RawFd requires an active tokio runtime")
            }
        }

        impl IntoRawFd for $ty {
            fn into_raw_fd(self) -> RawFd {
                self.extract_raw_fd()
            }
        }

        impl AsRawFd for $ty {
            fn as_raw_fd(&self) -> RawFd {
                self.inner.as_raw_fd()
            }
        }

        impl Drop for $ty {
            fn drop(&mut self) {
                if !self.dead.load(Ordering::SeqCst) {
                    unistd::close(self.as_raw_fd()).ok();
                }
            }
        }
    };
}

fd_impl!(RawReceiver);
fd_impl!(RawSender);

macro_rules! nix_eintr {
    ($expr:expr) => {
        loop {
            match $expr {
                Err(Errno::EINTR) => continue,
                other => break other,
            }
        }
    };
}

fn recv_impl(
    fd: RawFd,
    buf: &mut [u8],
    fds: Option<Vec<i32>>,
    fd_count: usize,
) -> io::Result<(usize, Option<Vec<RawFd>>)> {
    let iov = [IoVec::from_mut_slice(buf)];
    let mut new_fds = None;
    let msgspace_size = unsafe { CMSG_SPACE(mem::size_of::<RawFd>() as c_uint) * fd_count as u32 };
    let mut cmsgspace = vec![0u8; msgspace_size as usize];

    let msg = nix_eintr!(recvmsg(fd, &iov, Some(&mut cmsgspace), MSG_FLAGS))?;

    for cmsg in msg.cmsgs() {
        if let ControlMessageOwned::ScmRights(fds) = cmsg {
            if !fds.is_empty() {
                #[cfg(target_os = "macos")]
                unsafe {
                    for &fd in &fds {
                        // as per documentation this does not ever fail
                        // with EINTR
                        libc::ioctl(fd, libc::FIOCLEX);
                    }
                }
                new_fds = Some(fds);
            }
        }
    }

    if msg.bytes == 0 {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "could not read",
        ));
    }

    let fds = match (fds, new_fds) {
        (None, Some(new)) => Some(new),
        (Some(mut old), Some(new)) => {
            old.extend(new);
            Some(old)
        }
        (old, None) => old,
    };

    Ok((msg.bytes, fds))
}

fn send_impl(fd: RawFd, data: &[u8], fds: &[RawFd]) -> io::Result<usize> {
    let iov = [IoVec::from_slice(&data)];
    let sent = if !fds.is_empty() {
        nix_eintr!(sendmsg(
            fd,
            &iov,
            &[ControlMessage::ScmRights(fds)],
            MsgFlags::empty(),
            None,
        ))?
    } else {
        nix_eintr!(sendmsg(fd, &iov, &[], MsgFlags::empty(), None))?
    };
    if sent == 0 {
        return Err(io::Error::new(io::ErrorKind::WriteZero, "could not send"));
    }
    Ok(sent)
}

/// Creates a raw connected channel.
pub fn raw_channel() -> io::Result<(RawSender, RawReceiver)> {
    let (sender, receiver) = tokio::net::UnixStream::pair()?;
    Ok((
        RawSender::from_std(sender.into_std()?)?,
        RawReceiver::from_std(receiver.into_std()?)?,
    ))
}

/// An async raw receiver.
#[derive(Debug)]
pub struct RawReceiver {
    inner: AsyncFd<RawFd>,
    dead: AtomicBool,
}

impl RawReceiver {
    /// Connects a receiver to a named unix socket.
    pub async fn connect<P: AsRef<Path>>(p: P) -> io::Result<RawReceiver> {
        let stream = tokio::net::UnixStream::connect(p).await?;
        RawReceiver::from_std(stream.into_std()?)
    }

    /// Receives raw bytes from the socket.
    pub async fn recv(&self) -> io::Result<(Vec<u8>, Option<Vec<RawFd>>)> {
        let mut header = MsgHeader::default();
        self.recv_impl(header.as_buf_mut(), 0).await?;
        let mut buf = header.make_buffer();
        let (_, fds) = self.recv_impl(&mut buf, header.fd_count as usize).await?;
        Ok((buf, fds))
    }

    async fn recv_impl(
        &self,
        buf: &mut [u8],
        fd_count: usize,
    ) -> io::Result<(usize, Option<Vec<RawFd>>)> {
        let mut pos = 0;
        let mut fds = None;

        loop {
            let mut guard = self.inner.readable().await?;
            let (bytes, new_fds) = match guard
                .try_io(|inner| recv_impl(inner.as_raw_fd(), &mut buf[pos..], fds.take(), fd_count))
            {
                Ok(result) => result,
                Err(_would_block) => continue,
            }?;

            fds = new_fds;
            pos += bytes;
            if pos >= buf.len() {
                return Ok((pos, fds));
            }
        }
    }
}

unsafe impl Send for RawReceiver {}
unsafe impl Sync for RawReceiver {}

/// An async raw sender.
#[derive(Debug)]
pub struct RawSender {
    inner: AsyncFd<RawFd>,
    dead: AtomicBool,
}

impl RawSender {
    /// Sends raw bytes and fds.
    pub async fn send(&self, data: &[u8], fds: &[RawFd]) -> io::Result<usize> {
        let header = MsgHeader {
            payload_len: data.len() as u32,
            fd_count: fds.len() as u32,
        };
        self.send_impl(header.as_buf(), &[][..]).await?;
        self.send_impl(&data, fds).await
    }

    async fn send_impl(&self, data: &[u8], mut fds: &[RawFd]) -> io::Result<usize> {
        let mut pos = 0;
        loop {
            let mut guard = self.inner.writable().await?;
            let sent = match guard.try_io(|inner| send_impl(inner.as_raw_fd(), &data[pos..], fds)) {
                Ok(result) => result,
                Err(_would_block) => continue,
            }?;
            pos += sent;
            fds = &[][..];
            if pos >= data.len() {
                return Ok(pos);
            }
        }
    }
}

unsafe impl Send for RawSender {}
unsafe impl Sync for RawSender {}
