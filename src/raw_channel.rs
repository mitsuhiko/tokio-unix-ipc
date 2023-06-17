use std::io;
use std::io::{IoSlice, IoSliceMut};
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
use nix::unistd;

use tokio::io::unix::AsyncFd;

#[cfg(any(
    target_os = "android",
    target_os = "dragonfly",
    target_os = "freebsd",
    target_os = "linux",
    target_os = "netbsd",
    target_os = "openbsd"
))]
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

/// Data received via `SCM_CREDENTIALS` from a remote process.
#[derive(Debug, Clone)]
pub struct Credentials {
    pid: libc::pid_t,
    uid: libc::uid_t,
    gid: libc::gid_t,
}

impl Credentials {
    /// The remote process identifier.
    pub fn pid(&self) -> libc::pid_t {
        self.pid
    }

    /// The remote process user ID.
    pub fn uid(&self) -> libc::uid_t {
        self.uid
    }

    /// The remote process group ID.
    pub fn gid(&self) -> libc::gid_t {
        self.gid
    }
}

#[cfg(any(target_os = "android", target_os = "linux"))]
impl From<nix::sys::socket::UnixCredentials> for Credentials {
    fn from(c: nix::sys::socket::UnixCredentials) -> Self {
        Self {
            pid: c.pid(),
            uid: c.uid(),
            gid: c.gid(),
        }
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

            /// Convert from a standard stream.  This is a fallible
            /// operation because registering the file descriptor with
            /// the async runtime may fail.
            ///
            /// # Panics
            ///
            /// This function panics if it is not called from within a runtime with
            /// IO enabled.
            pub fn from_std(stream: UnixStream) -> io::Result<Self> {
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
    _want_creds: bool,
) -> io::Result<(usize, Option<Vec<RawFd>>, Option<Credentials>)> {
    let mut iov = [IoSliceMut::new(buf)];
    let mut new_fds = None;

    #[allow(unused_mut)]
    let mut creds = None;

    // Compute the size of ancillary data, combining expected number of file descriptors
    // with any space needed for credentials.
    let msgspace_size = {
        let fd_size = unsafe { CMSG_SPACE(mem::size_of::<RawFd>() as c_uint) * fd_count as u32 };
        #[cfg(any(target_os = "android", target_os = "linux"))]
        {
            let cred_size: u32 = _want_creds
                .then(|| unsafe {
                    CMSG_SPACE(mem::size_of::<nix::sys::socket::UnixCredentials>() as c_uint)
                })
                .unwrap_or_default();
            fd_size + cred_size
        }
        #[cfg(not(any(target_os = "android", target_os = "linux")))]
        {
            fd_size
        }
    };
    let mut cmsgspace = vec![0u8; msgspace_size as usize];

    let msg = nix_eintr!(recvmsg::<()>(fd, &mut iov, Some(&mut cmsgspace), MSG_FLAGS))?;

    for cmsg in msg.cmsgs() {
        match cmsg {
            ControlMessageOwned::ScmRights(fds) => {
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
            #[cfg(any(target_os = "android", target_os = "linux"))]
            ControlMessageOwned::ScmCredentials(c) => {
                creds = Some(c.into());
            }
            _ => {}
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

    Ok((msg.bytes, fds, creds))
}

#[cfg(any(target_os = "android", target_os = "linux"))]
fn send_impl(fd: RawFd, data: &[u8], fds: &[RawFd], creds: bool) -> io::Result<usize> {
    let iov = [IoSlice::new(&data)];
    let creds = creds.then(|| nix::sys::socket::UnixCredentials::new());
    let sent = match (fds, creds.as_ref()) {
        ([], None) => nix_eintr!(sendmsg::<()>(fd, &iov, &[], MsgFlags::empty(), None))?,
        ([], Some(creds)) => nix_eintr!(sendmsg::<()>(
            fd,
            &iov,
            &[ControlMessage::ScmCredentials(creds),],
            MsgFlags::empty(),
            None,
        ))?,
        (fds, Some(creds)) => {
            let cmsgs = &[
                ControlMessage::ScmRights(fds),
                ControlMessage::ScmCredentials(creds),
            ];
            nix_eintr!(sendmsg::<()>(fd, &iov, cmsgs, MsgFlags::empty(), None,))?
        }
        (fds, None) => {
            let cmsgs = &[ControlMessage::ScmRights(fds)];
            nix_eintr!(sendmsg::<()>(fd, &iov, cmsgs, MsgFlags::empty(), None,))?
        }
    };
    if sent == 0 {
        return Err(io::Error::new(io::ErrorKind::WriteZero, "could not send"));
    }
    Ok(sent)
}

#[cfg(not(any(target_os = "android", target_os = "linux")))]
fn send_impl(fd: RawFd, data: &[u8], fds: &[RawFd], _creds: bool) -> io::Result<usize> {
    let iov = [IoSlice::new(&data)];
    let sent = if !fds.is_empty() {
        nix_eintr!(sendmsg::<()>(
            fd,
            &iov,
            &[ControlMessage::ScmRights(fds)],
            MsgFlags::empty(),
            None,
        ))?
    } else {
        nix_eintr!(sendmsg::<()>(fd, &iov, &[], MsgFlags::empty(), None))?
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

/// Creates a raw connected channel from an already extant socket.
pub fn raw_channel_from_std(sender: UnixStream) -> io::Result<(RawSender, RawReceiver)> {
    let receiver = sender.try_clone()?;
    Ok((
        RawSender::from_std(sender)?,
        RawReceiver::from_std(receiver)?,
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
        self.recv_impl(header.as_buf_mut(), 0, false).await?;
        let mut buf = header.make_buffer();
        let (_, fds, _) = self
            .recv_impl(&mut buf, header.fd_count as usize, false)
            .await?;
        Ok((buf, fds))
    }

    /// Receives raw bytes and credentials from the socket.
    #[cfg(any(target_os = "android", target_os = "linux"))]
    pub async fn recv_with_credentials(
        &self,
    ) -> io::Result<(Vec<u8>, Option<Vec<RawFd>>, Credentials)> {
        nix::sys::socket::setsockopt(
            self.inner.as_raw_fd(),
            nix::sys::socket::sockopt::PassCred,
            &true,
        )?;
        let mut header = MsgHeader::default();
        let (_, _, creds) = self.recv_impl(header.as_buf_mut(), 0, true).await?;
        let creds = creds.ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "Remote did not provide credentials",
            )
        })?;
        let mut buf = header.make_buffer();
        let (_, fds, _) = self
            .recv_impl(&mut buf, header.fd_count as usize, false)
            .await?;
        Ok((buf, fds, creds))
    }

    async fn recv_impl(
        &self,
        buf: &mut [u8],
        fd_count: usize,
        want_creds: bool,
    ) -> io::Result<(usize, Option<Vec<RawFd>>, Option<Credentials>)> {
        let mut pos = 0;
        let mut fds = None;

        loop {
            let mut guard = self.inner.readable().await?;
            let (bytes, new_fds, creds) = match guard.try_io(|inner| {
                recv_impl(
                    inner.as_raw_fd(),
                    &mut buf[pos..],
                    fds.take(),
                    fd_count,
                    want_creds,
                )
            }) {
                Ok(result) => result,
                Err(_would_block) => continue,
            }?;

            fds = new_fds;
            pos += bytes;
            if pos >= buf.len() {
                return Ok((pos, fds, creds));
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
    #[allow(dead_code)]
    dead: AtomicBool,
}

impl RawSender {
    /// Sends raw bytes and fds.
    pub async fn send(&self, data: &[u8], fds: &[RawFd]) -> io::Result<usize> {
        let header = MsgHeader {
            payload_len: data.len() as u32,
            fd_count: fds.len() as u32,
        };
        self.send_impl(header.as_buf(), &[][..], false).await?;
        self.send_impl(&data, fds, false).await
    }

    /// Sends raw bytes and fds along with current process credentials.
    #[cfg(any(target_os = "android", target_os = "linux"))]
    pub async fn send_with_credentials(&self, data: &[u8], fds: &[RawFd]) -> io::Result<usize> {
        let header = MsgHeader {
            payload_len: data.len() as u32,
            fd_count: fds.len() as u32,
        };
        self.send_impl(header.as_buf(), &[][..], true).await?;
        self.send_impl(&data, fds, false).await
    }

    async fn send_impl(&self, data: &[u8], mut fds: &[RawFd], creds: bool) -> io::Result<usize> {
        let mut pos = 0;
        loop {
            let mut guard = self.inner.writable().await?;
            let sent = match guard
                .try_io(|inner| send_impl(inner.as_raw_fd(), &data[pos..], fds, creds))
            {
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
