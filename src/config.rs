use envconfig::Envconfig;

use std::error::Error;
use std::process::Command;

#[derive(Envconfig, Debug, Clone)]
pub struct Config {
    // CONNECTION SETTINGS
    #[envconfig(from = "IMAP_HOST")]
    pub host: String,

    #[envconfig(from = "IMAP_PORT")]
    pub port: u16,

    #[envconfig(from = "IMAP_USER")]
    pub user: String,

    #[envconfig(from = "IMAP_PASSCMD")]
    pub pass_cmd: String,

    #[envconfig(from = "IMAP_MAILBOX", default = "INBOX")]
    pub mailbox: String,

    // SYNC SETTINGS
    #[envconfig(from = "IMAP_MAILDIR", default = "~/Maildir")]
    pub maildir: String,

    #[envconfig(from = "IMAP_MBSYNC_PATH", default = "mbsync")]
    pub mbsync_path: String,

    #[envconfig(from = "IMAP_MBSYNC_CONF", default = "")]
    pub mbsync_conf: String,
}

impl Config {
    /// Execute the given command to get the password.
    pub fn get_password(&self) -> Result<String, Box<dyn Error>> {
        // put together the command
        let mut pass_cmd_it = self.pass_cmd.split(" ");
        let cmd = match pass_cmd_it.next() {
            Some(cmd) => cmd,
            None => return Err("Error parsing pass_cmd".into()),
        };
        let mut command = Command::new(cmd);
        for arg in pass_cmd_it {
            command.arg(arg);
        }

        // execute and return result
        let output = command.output()?;
        if !output.status.success() {
            return Err(format!("Command exited with code: {:?}", output.status.code()).into());
        }
        let s = std::str::from_utf8(&output.stdout)?;
        Ok(String::from(s.strip_suffix("\n").unwrap()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    fn get_default_config() -> Config {
        for (var, val) in vec![
            ("IMAP_HOST", "127.0.0.1"),
            ("IMAP_PORT", "666"),
            ("IMAP_USER", "user"),
            ("IMAP_PASSCMD", "echo super_secret_password"),
        ] {
            env::set_var(var, val);
        }

        Config::init().unwrap()
    }

    /// test the envconfig parsing
    #[test]
    fn test_config_defaults() {
        let config = get_default_config();

        assert_eq!(config.host, "127.0.0.1");
        assert_eq!(config.port, 666);
        assert_eq!(config.user, "user");
        assert_eq!(config.pass_cmd, "echo super_secret_password");
        assert_eq!(config.mailbox, "INBOX");
        assert_eq!(config.maildir, "~/Maildir");
        assert_eq!(config.mbsync_path, "mbsync");
        assert_eq!(config.mbsync_conf, "");
    }

    /// test the get_password function
    #[test]
    fn test_get_password() {
        let config = get_default_config();

        let password = config.get_password().unwrap();

        assert_eq!(password, "super_secret_password");
    }
}
