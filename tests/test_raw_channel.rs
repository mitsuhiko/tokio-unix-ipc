use std::fmt::Write;

use tokio_unix_ipc::raw_channel;

#[tokio::test]
async fn test_basic() {
    let (tx, rx) = raw_channel().unwrap();
    let local = tokio::task::LocalSet::new();

    local.spawn_local(async move {
        tx.send(b"Hello World!", &[][..]).await.unwrap();
    });

    local
        .run_until(async move {
            let (bytes, fds) = rx.recv().await.unwrap();
            assert_eq!(bytes, b"Hello World!");
            assert_eq!(fds, None);
        })
        .await;
}

#[tokio::test]
#[cfg(any(target_os = "android", target_os = "linux"))]
async fn test_creds() {
    let (tx, rx) = raw_channel().unwrap();

    let myuid = nix::unistd::getuid().as_raw() as libc::uid_t;
    let mypid = nix::unistd::getpid().as_raw() as libc::pid_t;

    let local = tokio::task::LocalSet::new();

    local.spawn_local(async move {
        tx.send_with_credentials(b"Hello World!", &[][..])
            .await
            .unwrap();
    });

    local
        .run_until(async move {
            let (bytes, fds, creds) = rx.recv_with_credentials().await.unwrap();
            assert_eq!(bytes, b"Hello World!");
            assert_eq!(fds, None);
            assert_eq!(creds.uid(), myuid);
            assert_eq!(creds.pid(), mypid);
        })
        .await;
}

#[tokio::test]
async fn test_large_buffer() {
    let mut buf = String::new();
    for x in 0..100000 {
        write!(&mut buf, "{}", x).ok();
    }

    let local = tokio::task::LocalSet::new();

    let (tx, rx) = raw_channel().unwrap();

    let server_buf = buf.clone();
    local.spawn_local(async move {
        tx.send(server_buf.as_bytes(), &[][..]).await.unwrap();
    });

    local
        .run_until(async move {
            let (bytes, fds) = rx.recv().await.unwrap();
            assert_eq!(bytes, buf.as_bytes());
            assert_eq!(fds, None);
        })
        .await;
}
