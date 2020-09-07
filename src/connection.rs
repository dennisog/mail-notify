use imap;
use native_tls::{TlsConnector, TlsStream};

use log::{debug, info, warn};

use std::error::Error;
use std::net::TcpStream;
use std::thread;
use std::time::Duration;

use super::config::Config;

type Result<T> = std::result::Result<T, Box<dyn Error>>;

pub type ImapSession = imap::Session<TlsStream<TcpStream>>;

pub struct Connection {
    // need to keep the config to be able to re-compute
    config: Config,

    // this is public so that the handler can query the mail server for more
    // information.
    pub session: Option<ImapSession>,
}

impl Connection {
    /// Create a new connection to the mail server. We store a copy of the
    /// config so that we can re-connect in emergencies.
    pub fn new(config: &Config) -> Self {
        Self {
            config: config.clone(),
            session: None,
        }
    }

    /// Connect to the mail server specified in the config.
    pub fn connect(&mut self) -> Result<()> {
        debug!(
            "Obtaining password with pass_cmd: '{}'",
            self.config.pass_cmd
        );
        let password = self.config.get_password()?;

        let tls = TlsConnector::builder().build()?;

        debug!("Connecting to server");
        let client = imap::connect(
            (self.config.host.as_str(), self.config.port),
            self.config.host.as_str(),
            &tls,
        )?;

        debug!("Logging in");
        let mut imap_session = client.login(&self.config.user, password).map_err(|e| e.0)?;
        imap_session.select(&self.config.mailbox)?;
        self.session = Some(imap_session);

        Ok(())
    }

    fn logout(&mut self) {
        if let Some(session) = &mut self.session {
            let _ = session.logout();
        }
    }

    /// Listen to updates from the server and execute the handler when one is
    /// received. If something goes wrong with the connection, attempt to
    /// reconnect a few times before giving up.
    pub fn listen<T>(&mut self, mut handler: T) -> Result<()>
    where
        T: FnMut(&mut ImapSession) -> Result<()>,
    {
        // this reconnect loop is inspired by jonhoo/buzz

        loop {
            match self.wait() {
                Ok(()) => match &mut self.session {
                    None => {
                        self.logout();
                        break;
                    }
                    Some(session) => {
                        if let Err(err) = handler(session) {
                            warn!("Error in handler: {}, reconnecting...", err);
                            self.logout();
                            break;
                        }
                    }
                },
                Err(e) => {
                    warn!("Connection error: {}", e);
                    // attempt to log out before reconnecting
                    self.logout();
                    break;
                }
            }
        }

        let mut wait = 1;
        for _ in 0..5 {
            info!("Attempting to reconnect");
            match self.connect() {
                Ok(()) => return self.listen(handler),
                Err(e) => {
                    warn!("Reconnect attempt failed: {}", e);
                    thread::sleep(Duration::from_secs(wait));
                }
            }
            wait *= 2;
        }
        // I don't know what this type of error-reporting is but it works?
        Err("Too many failed reconnect attempts".into())
    }

    /// Wait until the next update is received from the server. If anything
    /// goes wrong, return an error.
    fn wait(&mut self) -> Result<()> {
        match &mut self.session {
            Some(session) => {
                let status = session.idle()?;
                status.wait_keepalive()?; // blocks until something happens
                Ok(())
            }
            None => Err("Not connected.".into()),
        }
    }
}
