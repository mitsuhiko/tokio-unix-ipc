use std::fs::File;
use std::io::Read;
use std::os::unix::net::UnixStream;

use tokio_unix_ipc::serde::Handle;
use tokio_unix_ipc::{channel, channel_from_std, symmetric_channel, Receiver, Sender};

#[tokio::test]
async fn test_basic() {
    let f = Handle::from(std::fs::File::open("src/serde.rs").unwrap());

    let (tx, rx) = symmetric_channel().unwrap();

    tokio::spawn(async move {
        tx.send(f).await.unwrap();
    });

    let f = rx.recv().await.unwrap();

    let mut out = Vec::new();
    f.into_inner().read_to_end(&mut out).unwrap();
    assert!(out.len() > 100);
}

#[tokio::test]
async fn test_basic_asymmetric() {
    let f = Handle::from(std::fs::File::open("src/serde.rs").unwrap());

    let (tx, rx) = channel().unwrap();

    tokio::task::spawn_blocking(|| {
        let mut out = Vec::new();
        f.into_inner().read_to_end(&mut out).unwrap();
        tokio::runtime::Handle::current().block_on(async move { tx.send(out).await.unwrap() })
    });

    let out: Vec<u8> = rx.recv().await.unwrap();
    assert!(out.len() > 100);
}

#[tokio::test]
async fn test_send_channel() {
    let (tx, rx) = symmetric_channel().unwrap();
    let (sender, receiver) = symmetric_channel::<Handle<File>>().unwrap();

    tokio::spawn(async move {
        tx.send(sender).await.unwrap();
        let handle = receiver.recv().await.unwrap();
        let mut file = handle.into_inner();
        let mut out = Vec::new();
        file.read_to_end(&mut out).unwrap();
        assert!(out.len() > 100);
    });

    let sender = rx.recv().await.unwrap();
    sender
        .send(Handle::from(File::open("src/serde.rs").unwrap()))
        .await
        .unwrap();
}

#[tokio::test]
async fn test_multiple_fds() {
    let (tx1, rx1) = symmetric_channel().unwrap();
    let (tx2, rx2) = symmetric_channel::<()>().unwrap();
    let (tx3, rx3) = symmetric_channel::<()>().unwrap();

    let a = tokio::spawn(async move {
        tx1.send((tx2, rx2, tx3, rx3)).await.unwrap();
    });

    let b = tokio::spawn(async move {
        let _channels = rx1.recv().await.unwrap();
    });

    a.await.unwrap();
    b.await.unwrap();
}

#[tokio::test]
async fn test_conversion() {
    let (tx, rx) = symmetric_channel::<i32>().unwrap();
    let raw_tx = tx.into_raw_sender();
    let raw_rx = rx.into_raw_receiver();
    let tx = Sender::<bool>::from(raw_tx);
    let rx = Receiver::<bool>::from(raw_rx);

    let a = tokio::spawn(async move {
        tx.send(true).await.unwrap();
    });

    let b = tokio::spawn(async move {
        assert_eq!(rx.recv().await.unwrap(), true);
    });

    a.await.unwrap();
    b.await.unwrap();
}

#[tokio::test]
async fn test_zero_sized_type() {
    let (tx, rx) = symmetric_channel::<()>().unwrap();

    tokio::spawn(async move {
        tx.send(()).await.unwrap();
    });

    rx.recv().await.unwrap();
}

const HELO: &str = "HELO from server";
const SUP: &str = "SUP from client";
const MORE: &str = "ANOTHER msg from client";
const BYE: &str = "BYE msg from server";

async fn run_client(sock: UnixStream) {
    let (send, recv) = channel_from_std(sock).unwrap();
    send.send(SUP.to_string()).await.unwrap();
    let msg: String = recv.recv().await.unwrap();
    assert_eq!(msg.as_str(), HELO);
    for _ in 0..3 {
        send.send(MORE.to_string()).await.unwrap();
    }
    let msg = recv.recv().await.unwrap();
    assert_eq!(msg.as_str(), BYE);
    // Drop send+recv, closing the connection
}

async fn run_server(sock: UnixStream) {
    let (send, recv) = channel_from_std(sock).unwrap();
    let msg: String = recv.recv().await.unwrap();
    assert_eq!(msg.as_str(), SUP);
    send.send(HELO.to_string()).await.unwrap();
    for _ in 0..3 {
        let msg: String = recv.recv().await.unwrap();
        assert_eq!(msg.as_str(), MORE);
    }
    send.send(BYE.to_string()).await.unwrap();
    match recv.recv().await {
        Ok(msg) => panic!("Unexpected message {}", msg),
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {}
        Err(e) => {
            panic!("Unexpected error {:?}", e)
        }
    }
}

/// This test simulates a multi-process client server where we've allocated
/// a socket pair externally (e.g. systemd socket activation).
#[test]
fn test_pair_from_std() {
    let mkrt = || {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
    };
    let (client, server) = std::os::unix::net::UnixStream::pair().unwrap();
    let client_thread =
        std::thread::spawn(move || mkrt().block_on(async move { run_client(client).await }));

    mkrt().block_on(async move { run_server(server).await });

    client_thread.join().unwrap();
}
