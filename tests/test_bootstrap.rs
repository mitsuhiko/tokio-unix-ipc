use tokio_unix_ipc::{symmetric_channel, Bootstrapper, Receiver, Sender};

#[tokio::test]
async fn test_bootstrap() {
    let bootstrapper = Bootstrapper::new().unwrap();
    let path = bootstrapper.path().to_owned();

    let local = tokio::task::LocalSet::new();

    let send = local.spawn_local(async move {
        bootstrapper.send(42u32).await.unwrap();
        bootstrapper.send(23u32).await.unwrap();
    });

    local
        .run_until(async move {
            let receiver = Receiver::<u32>::connect(path).await.unwrap();
            let a = receiver.recv().await.unwrap();
            let b = receiver.recv().await.unwrap();
            assert_eq!(a + b, 65);
        })
        .await;
    send.await.unwrap();
}

#[tokio::test]
async fn test_bootstrap_reverse() {
    let bootstrapper = Bootstrapper::new().unwrap();
    let path = bootstrapper.path().to_owned();
    let (tx, rx) = symmetric_channel::<u32>().unwrap();

    let local = tokio::task::LocalSet::new();

    local.spawn_local(async move {
        let receiver = Receiver::<Sender<u32>>::connect(path).await.unwrap();
        let result_sender = receiver.recv().await.unwrap();
        result_sender.send(42 + 23).await.unwrap();
    });

    local
        .run_until(async move {
            bootstrapper.send(tx).await.unwrap();
            assert_eq!(rx.recv().await.unwrap(), 65);
        })
        .await;
}
