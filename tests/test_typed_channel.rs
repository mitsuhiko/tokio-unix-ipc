use std::fs::File;
use std::io::Read;

use tokio_unix_ipc::{channel, Handle, Receiver, Sender};

#[tokio::test]
async fn test_basic() {
    let f = Handle::from(std::fs::File::open("src/serde.rs").unwrap());

    let (tx, rx) = channel().unwrap();

    tokio::spawn(async move {
        tx.send(f).await.unwrap();
    });

    let f = rx.recv().await.unwrap();

    let mut out = Vec::new();
    f.into_inner().read_to_end(&mut out).unwrap();
    assert!(out.len() > 100);
}

#[tokio::test]
async fn test_send_channel() {
    let (tx, rx) = channel().unwrap();
    let (sender, receiver) = channel::<Handle<File>>().unwrap();

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
    let (tx1, rx1) = channel().unwrap();
    let (tx2, rx2) = channel::<()>().unwrap();
    let (tx3, rx3) = channel::<()>().unwrap();

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
    let (tx, rx) = channel::<i32>().unwrap();
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
    let (tx, rx) = channel::<()>().unwrap();

    tokio::spawn(async move {
        tx.send(()).await.unwrap();
    });

    rx.recv().await.unwrap();
}
