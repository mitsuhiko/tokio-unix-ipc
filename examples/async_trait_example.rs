use serde_::{Deserialize, Serialize};
use std::{env, process};
use std::sync::{Arc};
use tokio_unix_ipc::{Bootstrapper, channel, Receiver, Sender};

const ENV_VAR: &str = "PROC_CONNECT_TO";

#[derive(Serialize, Deserialize)]
#[serde(crate = "serde_")]
pub enum Task {
    Sum(Vec<i64>, Sender<i64>),
    Shutdown
}

#[async_trait::async_trait]
pub trait TaskService {
    async fn sum(&self, vec: Vec<i64>) -> i64;
    async fn shutdown(&self);
}

#[tokio::main]
async fn main() {
    if let Ok(path) = env::var(ENV_VAR) {
        let server = IpcReceiver::new(path).await;
        server.start().await;
    } else {
        let client = IpcSender::new();
        let mut child = process::Command::new(env::current_exe().unwrap())
            .env(ENV_VAR, client.get_path().await)
            .spawn()
            .unwrap();

        let sum = client.sum(vec![1, 2, 3]).await;
        println!("sum = {}", sum);

        child.kill().ok();
        child.wait().ok();
    }
}


/// ipc_receiver
pub struct IpcReceiver {
    pub recv: Receiver<Task>,
}
impl IpcReceiver {
    pub async fn new(path: String) -> Self {
        let receiver = Receiver::<Task>::connect(path).await.unwrap();
        IpcReceiver {
            recv: receiver,
        }
    }
    pub async fn start(&self) {
        loop {
            let task = self.recv.recv().await.unwrap();
            match task {
                Task::Sum(vec, tx) => {
                    let sum = self.sum(vec).await;
                    tx.send(sum).await.unwrap();
                },
                Task::Shutdown => {
                    self.shutdown().await;
                    break
                }
            }
        }
    }
}
#[async_trait::async_trait]
impl TaskService for IpcReceiver {
    async fn sum(&self, vec: Vec<i64>) -> i64 {
        vec.into_iter().sum::<i64>()
    }

    async fn shutdown(&self) {
    }
}

/// ipc_sender
pub struct IpcSender {
    pub bootstrap: Arc<Bootstrapper>,
}
impl IpcSender {
    pub fn new() -> Self {
        let bootstrapper = Bootstrapper::new().unwrap();
        IpcSender {
            bootstrap: Arc::new(bootstrapper)
        }
    }
    pub async fn get_path(&self) -> String {
        String::from(self.bootstrap.get_path())
    }
}
#[async_trait::async_trait]
impl TaskService for IpcSender {
    async fn sum(&self, vec: Vec<i64>) -> i64 {
        let (tx, rx) = channel().unwrap();
        self.bootstrap.send(Task::Sum(vec, tx)).await.unwrap();
        rx.recv().await.unwrap()
    }

    async fn shutdown(&self) {
        self.bootstrap.send(Task::Shutdown).await.unwrap()
    }
}