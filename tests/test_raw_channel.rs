use std::fmt::Write;

use tokio_unix_ipc::raw_channel;

#[tokio::test]
async fn test_basic() {
    let (tx, rx) = raw_channel().unwrap();

    tokio::spawn(async move {
        tx.send(b"Hello World!", &[][..]).await.unwrap();
    });

    let (bytes, fds) = rx.recv().await.unwrap();
    assert_eq!(bytes, b"Hello World!");
    assert_eq!(fds, None);
}

#[tokio::test]
async fn test_large_buffer() {
    let mut buf = String::new();
    for x in 0..100000 {
        write!(&mut buf, "{}", x).ok();
    }

    let (tx, rx) = raw_channel().unwrap();

    let server_buf = buf.clone();
    tokio::spawn(async move {
        tx.send(server_buf.as_bytes(), &[][..]).await.unwrap();
    });

    let (bytes, fds) = rx.recv().await.unwrap();
    assert_eq!(bytes, buf.as_bytes());
    assert_eq!(fds, None);
}
