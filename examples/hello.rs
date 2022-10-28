use serde_::{Deserialize, Serialize};
use std::env;
use std::process;
use tokio_unix_ipc::{symmetric_channel, Bootstrapper, Receiver, Sender};

const ENV_VAR: &str = "PROC_CONNECT_TO";

#[derive(Serialize, Deserialize, Debug)]
#[serde(crate = "serde_")]
pub enum Task {
    Sum(Vec<i64>, Sender<i64>),
    Shutdown,
}

#[tokio::main]
async fn main() {
    if let Ok(path) = env::var(ENV_VAR) {
        let receiver = Receiver::<Task>::connect(path).await.unwrap();
        loop {
            let task = receiver.recv().await.unwrap();
            match dbg!(task) {
                Task::Sum(values, tx) => {
                    tx.send(values.into_iter().sum::<i64>()).await.unwrap();
                }
                Task::Shutdown => break,
            }
        }
    } else {
        let bootstrapper = Bootstrapper::new().unwrap();
        let mut child = process::Command::new(env::current_exe().unwrap())
            .env(ENV_VAR, bootstrapper.path())
            .spawn()
            .unwrap();

        let (tx, rx) = symmetric_channel().unwrap();
        bootstrapper
            .send(Task::Sum(vec![23, 42], tx))
            .await
            .unwrap();
        println!("result: {}", rx.recv().await.unwrap());

        let (tx, rx) = symmetric_channel().unwrap();
        bootstrapper
            .send(Task::Sum((0..10).collect(), tx))
            .await
            .unwrap();
        println!("result: {}", rx.recv().await.unwrap());

        bootstrapper.send(Task::Shutdown).await.unwrap();

        child.kill().ok();
        child.wait().ok();
    }
}
