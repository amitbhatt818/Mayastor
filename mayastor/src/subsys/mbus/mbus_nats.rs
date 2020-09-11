//! NATS message bus connecting mayastor to the control plane components.
//!
//! It is designed to make sending events to control plane easy in the future.

use super::MessageBus;
use async_trait::async_trait;
use nats::asynk::Connection;
use once_cell::sync::OnceCell;
use smol::io;

pub(super) static NATS_MSG_BUS: OnceCell<NatsMessageBus> = OnceCell::new();
pub(super) fn message_bus_init(server: String) {
    std::thread::spawn(move || {
        NATS_MSG_BUS.get_or_init(|| {
            smol::block_on(async { NatsMessageBus::new(&server).await })
        });
    });
}

// Would we want to have both sync and async clients?
pub struct NatsMessageBus {
    connection: Connection,
}
impl NatsMessageBus {
    pub async fn connect(server: &str) -> Connection {
        info!("Connecting to the nats server {}...", server);
        // We retry in a loop until successful. Once connected the nats
        // library will handle reconnections for us.
        let interval = std::time::Duration::from_millis(500);
        let mut log_error = true;
        loop {
            match nats::asynk::connect(server).await {
                Ok(connection) => {
                    info!(
                        "Successfully connected to the nats server {}",
                        server
                    );
                    return connection;
                }
                Err(error) => {
                    if log_error {
                        warn!(
                            "Error connection: {}. Quietly retrying...",
                            error
                        );
                        log_error = false;
                    }
                    smol::Timer::after(interval).await;
                    continue;
                }
            }
        }
    }

    async fn new(server: &str) -> Self {
        Self {
            connection: Self::connect(server).await,
        }
    }
}

#[async_trait(?Send)]
impl MessageBus for NatsMessageBus {
    async fn fire(
        &self,
        channel: &str,
        message: impl AsRef<[u8]> + 'async_trait,
    ) -> std::io::Result<()> {
        self.connection.publish(channel, message).await
    }
    async fn flush(&self) -> io::Result<()> {
        self.connection.flush().await
    }

    async fn wait_for_connection() {
        let interval = std::time::Duration::from_millis(500);
        let mut log_error = true;
        loop {
            match NATS_MSG_BUS.get() {
                Some(_) => {
                    info!("Successfully connected to the nats server");
                    break;
                }
                None => {
                    if log_error {
                        warn!("Message bus not ready, quietly retrying...");
                        log_error = true;
                    }
                    smol::Timer::after(interval).await;
                    continue;
                }
            }
        }
    }
}
