#![feature(async_closure)]

use futures_util::StreamExt;
use log::info;
use nats::{self, asynk as anats};
use structopt::StructOpt;

#[derive(Debug, StructOpt)]
struct Options {
    /// The Nats Server URL to connect to
    /// (supports the nats schema)
    /// Default: nats://127.0.0.1:4222
    #[structopt(long, short, default_value = "nats://127.0.0.1:4222")]
    url: String,

    /// Channel to dump string messages from
    #[structopt(long, short, default_value = "register")]
    channel: String,
}

#[tokio::main]
async fn main() {
    env_logger::init_from_env(
        env_logger::Env::default()
            .filter_or(env_logger::DEFAULT_FILTER_ENV, "info"),
    );

    let options = Options::from_args();
    info!("Using options: {:?}", &options);

    let nc = anats::connect(&options.url).await.unwrap();
    let sub = nc.subscribe(&options.channel).await.unwrap();

    nc.publish(&options.channel, "Self Check").await.unwrap();

    let rtt = nc.rtt().await.unwrap();
    info!("RTT: {:?}", rtt);

    sub.for_each(async move |message| {
        info!(
            "Received message: {}",
            std::str::from_utf8(&message.data).unwrap()
        );
    })
    .await;
}
