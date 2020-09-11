//! NATS message bus connecting mayastor to the control plane components.
//!
//! It is designed to make sending events to control plane easy in the future.
//! That's the reason for global sender protected by the mutex, that normally
//! would not be needed and currently is used only to terminate the message bus.

use async_trait::async_trait;
use futures::{select, FutureExt, StreamExt};
use once_cell::sync::OnceCell;
use serde::{Deserialize, Serialize};
use smol::io;
use snafu::Snafu;
use std::{env, time::Duration};

pub mod mbus_nats;
use mbus_nats::message_bus_init;

use crate::core::{MayastorCliArgs, MayastorEnvironment};
use spdk_sys::{
    spdk_subsystem,
    spdk_subsystem_fini_next,
    spdk_subsystem_init_next,
};

use crate::subsys::mbus::mbus_nats::{NatsMessageBus, NATS_MSG_BUS};
use structopt::StructOpt;

// wrapper around our MBUS subsystem used for registration
pub struct MessageBusSubsystem(pub(crate) *mut spdk_subsystem);

impl Default for MessageBusSubsystem {
    fn default() -> Self {
        Self::new()
    }
}

impl MessageBusSubsystem {
    /// initialize a new subsystem that handles the control plane
    /// message bus
    extern "C" fn init() {
        debug!("mayastor mbus subsystem init");

        let args = MayastorEnvironment::new(MayastorCliArgs::from_args());
        if let (Some(_), Some(grpc)) = (args.mbus_endpoint, args.grpc_endpoint)
        {
            Registration::init(&args.name, &grpc);
        }

        unsafe { spdk_subsystem_init_next(0) }
    }

    extern "C" fn fini() {
        debug!("mayastor mbus subsystem fini");
        let args = MayastorEnvironment::new(MayastorCliArgs::from_args());
        if args.mbus_endpoint.is_some() && args.grpc_endpoint.is_some() {
            Registration::get().fini();
        }
        unsafe { spdk_subsystem_fini_next() }
    }

    pub fn new() -> Self {
        println!("creating Mayastor mbus subsystem...");
        let args = MayastorEnvironment::new(MayastorCliArgs::from_args());
        if let Some(url) = args.mbus_endpoint {
            message_bus_init(url);
        }
        let mut ss = Box::new(spdk_subsystem::default());
        ss.name = b"mayastor_mbus\x00" as *const u8 as *const libc::c_char;
        ss.init = Some(Self::init);
        ss.fini = Some(Self::fini);
        // we could write out the config, I guess
        ss.write_config_json = None;
        Self(Box::into_raw(ss))
    }
}

/// Mayastor sends registration messages in this interval (kind of heart-beat)
const HB_INTERVAL: Duration = Duration::from_secs(2);

/// Errors for pool operations.
///
/// Note: The types here that would be normally used as source for snafu errors
/// do not implement Error trait required by Snafu. So they are renamed to
/// "cause" attribute and we use .map_err() instead of .context() when creating
/// them.
#[derive(Debug, Snafu)]
pub enum Error {
    #[snafu(display(
        "Failed to connect to the NATS server {}: {:?}",
        server,
        cause
    ))]
    ConnectFailed {
        cause: std::io::Error,
        server: String,
    },
    #[snafu(display(
        "Cannot issue requests if message bus hasn't been started"
    ))]
    NotStarted {},
    #[snafu(display("Failed to queue register request: {:?}", cause))]
    QueueRegister { cause: std::io::Error },
    #[snafu(display("Failed to queue deregister request: {:?}", cause))]
    QueueDeregister { cause: std::io::Error },
}

/// Register message payload
#[derive(Serialize, Deserialize, Debug)]
struct RegisterArgs {
    id: String,
    #[serde(rename = "grpcEndpoint")]
    grpc_endpoint: String,
}

/// Deregister message payload
#[derive(Serialize, Deserialize, Debug)]
struct DeregisterArgs {
    id: String,
}

#[derive(Clone)]
struct Configuration {
    /// Name of the node that mayastor is running on
    node: String,
    /// gRPC endpoint of the server provided by mayastor
    grpc_endpoint: String,
    /// heartbeat interval (how often the register message is sent)
    hb_interval: Duration,
}

#[derive(Clone)]
struct Registration {
    /// Configuration of the registration
    config: Configuration,
    /// Receive channel for messages and termination
    rcv_chan: smol::channel::Receiver<()>,
    /// Termination channel
    fini_chan: smol::channel::Sender<()>,
}

static MESSAGE_BUS_REG: OnceCell<Registration> = OnceCell::new();
impl Registration {
    pub fn init(node: &str, grpc_endpoint: &str) {
        MESSAGE_BUS_REG.get_or_init(|| Registration::new(node, grpc_endpoint));

        // spawn a runner thread responsible for registering and
        // deregistering the mayastor instance on shutdown
        std::thread::spawn(|| {
            smol::block_on(async {
                Self::get().clone().run().await;
            });
        });
    }

    fn new(node: &str, grpc_endpoint: &str) -> Registration {
        let (msg_sender, msg_receiver) = smol::channel::unbounded::<()>();
        let config = Configuration {
            node: node.to_owned(),
            grpc_endpoint: grpc_endpoint.to_owned(),
            hb_interval: match env::var("MAYASTOR_HB_INTERVAL")
                .map(|v| v.parse::<u64>())
            {
                Ok(Ok(num)) => Duration::from_secs(num),
                _ => HB_INTERVAL,
            },
        };
        Self {
            config,
            rcv_chan: msg_receiver,
            fini_chan: msg_sender,
        }
    }

    pub fn get() -> &'static Registration {
        MESSAGE_BUS_REG.get().unwrap()
    }

    pub fn fini(&self) {
        self.fini_chan.close();
    }
}

#[async_trait(?Send)]
pub trait MessageBus {
    ///// Fire an event - fire and forget
    async fn fire(
        &self,
        channel: &str,
        message: impl AsRef<[u8]> + 'async_trait,
    ) -> std::io::Result<()>;
    // /// Send an event - make sure it was received
    // async fn send(message: String) -> Result<(),()>;
    // /// Make a request and wait for a reply
    // async fn request(message: String) -> Result<String,()>;
    async fn flush(&self) -> io::Result<()>;

    async fn wait_for_connection();
}

impl Registration {
    /// Connect to the server and start emitting periodic register
    /// messages.
    /// Runs until the sender side of the message channel is closed
    pub async fn run(&mut self) {
        wait_for_connection().await;
        info!(
            "Registering '{}' and grpc server {} ...",
            self.config.node, self.config.grpc_endpoint
        );
        loop {
            if let Err(err) = self.register().await {
                error!("Registration failed: {:?}", err);
            };

            select! {
                _ = smol::Timer::after(self.config.hb_interval).fuse() => continue,
                msg = self.rcv_chan.next().fuse() => {
                    match msg {
                        Some(_) => log::info!("Messages have not been implemented yet"),
                        _ => {
                            log::info!("Terminating the NATS client");
                            break;
                        }
                    }
                }
            };
        }
        if let Err(err) = self.deregister().await {
            error!("Deregistration failed: {:?}", err);
        };
    }

    /// Send a register message to the NATS server.
    async fn register(&self) -> Result<(), Error> {
        let payload = RegisterArgs {
            id: self.config.node.clone(),
            grpc_endpoint: self.config.grpc_endpoint.clone(),
        };
        message_bus()
            .fire("register", serde_json::to_vec(&payload).unwrap())
            .await
            .map_err(|cause| Error::QueueRegister {
                cause,
            })?;

        // Note that the message was only queued and we don't know if it was
        // really sent to the message server
        // We could explicitly flush to make sure it reaches the server or
        // use request/reply to guarantee that it was delivered
        debug!(
            "Registered '{}' and grpc server {}",
            self.config.node, self.config.grpc_endpoint
        );
        Ok(())
    }

    /// Send a deregister message to the NATS server.
    async fn deregister(&self) -> Result<(), Error> {
        let payload = DeregisterArgs {
            id: self.config.node.clone(),
        };
        message_bus()
            .fire("deregister", serde_json::to_vec(&payload).unwrap())
            .await
            .map_err(|cause| Error::QueueDeregister {
                cause,
            })?;
        if let Err(e) = message_bus().flush().await {
            error!("Failed to explicitly flush: {}", e);
        }

        info!(
            "Deregistered '{}' and grpc server {}",
            self.config.node, self.config.grpc_endpoint
        );
        Ok(())
    }
}

pub fn message_bus() -> &'static impl MessageBus {
    NATS_MSG_BUS.get().unwrap()
}

pub async fn wait_for_connection() {
    <NatsMessageBus as MessageBus>::wait_for_connection().await;
}
