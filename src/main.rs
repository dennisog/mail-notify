#[macro_use]
extern crate envconfig_derive;
extern crate envconfig;
use envconfig::Envconfig;
mod config;

extern crate pretty_env_logger;
#[macro_use]
extern crate log;

mod connection;
use connection::Connection;

mod handler;
use handler::Handler;

use std::process;

fn main() {
    pretty_env_logger::init();

    info!("Loading config");
    let conf = config::Config::init().unwrap_or_else(|err| {
        warn!("Error initializing config: {:?}", err);
        process::exit(1);
    });
    debug!("Loaded configuration: {:?}", conf);

    info!("Connecting to server");
    let mut connection = Connection::new(&conf);

    connection.connect().unwrap_or_else(|err| {
        warn!("Error connecting to server: {:?}", err);
        process::exit(1);
    });
    info!("Successfully connected");

    debug!("Setting up handler");
    let mut handler = Handler::new(&conf).unwrap_or_else(|err| {
        warn!("Error creating handler: {:?}", err);
        process::exit(1);
    });

    info!("Listening for updates");
    connection
        .listen(move |c| handler.handle(c))
        .unwrap_or_else(|err| {
            warn!("Error while listening for updates: {}", err);
        });
}
