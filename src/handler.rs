use std::error::Error;
use std::fs;
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime};

use dbus::blocking::Connection;
use mailparse::{parse_headers, MailHeaderMap};
use notify_rust::{Notification, Timeout};
use rodio::Source;
use rust_embed::RustEmbed;
use shellexpand;
use sysinfo::{RefreshKind, SystemExt};
use walkdir::WalkDir;

use crate::config::Config;
use crate::connection::ImapSession;

use log::{debug, info, warn};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

pub struct Handler {
    notifier: Notifier,
    mbsync: Mbsync,
    last_notified: u32,
}

impl Handler {
    pub fn new(config: &Config) -> Result<Self> {
        let notifier = Notifier::new(config.maildir.as_str(), config.mailbox.as_str())?;
        let mbsync = Mbsync::new(config.mbsync_path.as_str(), config.mbsync_conf.as_str())?;
        Ok(Self {
            notifier,
            mbsync,
            last_notified: 0,
        })
    }

    pub fn handle(&mut self, session: &mut ImapSession) -> Result<()> {
        // get the most recent UID on the server
        let latest_uid = session.uid_search("UID *")?;

        // keep track of the last uid that we notified
        if let Some(uid) = latest_uid.into_iter().next() {
            if uid > self.last_notified {
                self.last_notified = uid;
                self.sync_and_notify();
            } else {
                debug!("Got update, but already notified for this UID");
            }
        }

        Ok(())
    }

    fn sync_and_notify(&self) {
        if let Err(e) = self.mbsync.synchronize() {
            panic!("Couldn't synchronize: {}", e);
        }
        if let Err(e) = self.notifier.notify() {
            warn!("Error notifying: {:?}", e);
        }
    }
}

struct Notifier {
    path: PathBuf,
    emacs: Emacs,
    sound: SoundNotifier,
}

impl Notifier {
    pub fn new(maildir: &str, mailbox: &str) -> Result<Self> {
        // get the right path for the maildir
        let expanded = shellexpand::tilde(maildir).into_owned();
        let path = Path::new(expanded.as_str()).join(mailbox);
        let path = path.canonicalize()?;

        // set up the emacs connection and the wav player
        let emacs = Emacs::new()?;
        let sound = SoundNotifier::new()?;

        Ok(Self { path, emacs, sound })
    }

    pub fn notify(&self) -> Result<()> {
        info!("Got new mail, notifying...");

        // get the newest message from the maildir
        let path = self
            .get_newest_message()
            .ok_or("Couldn't find most recent message!")?;

        // send desktop notification
        let MailMetadata { from, subject } = MailMetadata::new(path)?;
        Notification::new()
            .summary(from.as_str())
            .body(subject.as_str())
            .icon("mail-unread")
            .appname("You've got mail!")
            .timeout(Timeout::Milliseconds(5000))
            .show()?;

        // notify emacs
        self.emacs.notify()?;

        // play audio
        self.sound.play()?;

        Ok(())
    }

    fn get_newest_message(&self) -> Option<PathBuf> {
        let one_min_ago = SystemTime::now() - Duration::from_secs(60);

        let mut files: Vec<_> = WalkDir::new(self.path.as_os_str())
            .into_iter()
            .filter_map(|e| match e {
                // only look at files that I have permissions for. only add
                // them if they are non-hidden plain files and are younger than a minute
                // ago.
                Ok(entry) if !is_hidden(&entry) => match entry.metadata() {
                    Ok(md) if md.is_file() => match md.created() {
                        Ok(time) if time >= one_min_ago => Some(entry),
                        _ => None,
                    },
                    _ => None,
                },
                _ => None,
            })
            .collect();

        // sort the remaining files to get the most recent
        files.sort_by(|a, b| {
            let a = a.metadata().unwrap().created().unwrap();
            let b = b.metadata().unwrap().created().unwrap();
            a.cmp(&b)
        });

        // return the path to the most recent if it exists. otherwise return
        // None.
        files.pop().and_then(|e| Some(e.into_path()))
    }
}

fn is_hidden(entry: &walkdir::DirEntry) -> bool {
    entry
        .file_name()
        .to_str()
        .map(|s| s.starts_with("."))
        .unwrap_or(false)
}

struct MailMetadata {
    from: String,
    subject: String,
}

impl MailMetadata {
    fn new(path: PathBuf) -> Result<Self> {
        let contents = fs::read(path)?;
        let (headers, _) = parse_headers(&contents)?;
        let from = headers
            .get_first_value("From")
            .unwrap_or(String::from("Unknown Sender (parse error)"));
        let subject = headers
            .get_first_value("Subject")
            .unwrap_or(String::from("Unknown Subject (parse error)"));
        Ok(Self { from, subject })
    }
}

struct Mbsync {
    command: String,
    config_path: Option<PathBuf>,
}

impl Mbsync {
    pub fn new(command: &str, config_path: &str) -> Result<Self> {
        Ok(Self {
            command: String::from(command),
            config_path: if config_path == "" {
                None
            } else {
                let expanded = shellexpand::tilde(config_path).into_owned();
                Some(Path::new(expanded.as_str()).canonicalize()?)
            },
        })
    }

    fn command(&self) -> Command {
        let mut command = Command::new(&self.command);
        command.arg("-a");
        command.arg("-V");
        if let Some(cfgpath) = &self.config_path {
            command.arg("-c").arg(cfgpath);
        }
        command
    }

    pub fn synchronize(&self) -> Result<()> {
        // wait for running Mbsync processes
        self.wait();

        // run the mbsync process
        let mut cmd = self.command();
        info!("Running mbsync command: {:?}", cmd);
        let out = cmd.status()?;

        // check the output
        if out.success() {
            Ok(())
        } else {
            Err(format!("Command {:?} exited with status: {:?}", cmd, out).into())
        }
    }

    /// wait until all running processes with the same name as `self.command'
    /// are done.
    fn wait(&self) {
        let cmdname = Path::new(&self.command)
            .file_stem()
            .unwrap()
            .to_str()
            .unwrap();
        let mut system = sysinfo::System::new_with_specifics(RefreshKind::new().with_processes());
        loop {
            system.refresh_processes();
            let running_pids = system.get_process_by_name(cmdname);
            if running_pids.len() == 0 {
                break;
            }
        }
    }
}

struct Emacs {}

impl Emacs {
    pub fn new() -> Result<Self> {
        Ok(Self {})
    }

    /// use d-bus to notify Emacs
    pub fn notify(&self) -> Result<()> {
        debug!("Notifying Emacs");
        let conn = Connection::new_session()?;
        // I am registering these methods using some elisp code.
        let proxy = conn.with_proxy("net.ogbe.emacs", "/mail", Duration::from_millis(5000));
        let _ = proxy.method_call("net.ogbe.emacs.mail", "reindex", ())?;
        let _ = proxy.method_call("net.ogbe.emacs.mail", "refresh", ())?;
        Ok(())
    }
}

// I use RustEmbed to save the wav notifications in the binary. I only have to
// give the folder relative to the project root, and everything else works.
#[derive(RustEmbed)]
#[folder = "src/blob/"]
struct Asset;

struct SoundNotifier {}

impl SoundNotifier {
    pub fn new() -> Result<Self> {
        Ok(Self {})
    }

    // I would like to use pure rust library like rodio here, but
    // unfortunately, due to an issue [1], this code leaves a lingering audio
    // thread which uses too much CPU for my liking. So for now, we just pipe
    // the bytes to aplay, see the function below.
    //
    // https://github.com/RustAudio/rodio/issues/299
    pub fn _play_rodio(&self) -> Result<()> {
        let device = rodio::default_output_device().ok_or("Couldn't find output device!")?;
        // I wonder whether I can cache some of these below operations? I
        // attempted to store some of the intermediate things (the cursor, the
        // src, etc. in the SoundNotifier struct, but ran into a whole bunch of
        // problems.
        let bytes = Asset::get("snd1.wav").ok_or("Couldn't get sound asset")?;
        let cursor = Cursor::new(bytes);
        let src = rodio::Decoder::new(cursor)?;
        rodio::play_raw(&device, src.convert_samples());
        Ok(())
    }

    pub fn _play_aplay(&self) -> Result<()> {
        let bytes = Asset::get("snd1.wav").ok_or("Couldn't get sound asset")?;
        let mut proc = Command::new("aplay")
            .arg("-")
            .stdin(Stdio::piped())
            .spawn()?;
        {
            // need new scope because stdin is borrowing the proc?
            let stdin = proc.stdin.as_mut().ok_or("Couldn't open stdin")?;
            stdin.write_all(&*bytes)?;
        }
        proc.wait()?;
        Ok(())
    }

    pub fn play(&self) -> Result<()> {
        debug!("Playing sound");
        self._play_aplay()
    }
}
