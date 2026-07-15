//! Command-line parsing.
//!
//! ```text
//! DesktopDrawers                       -> Manager
//! DesktopDrawers --manage              -> Manager
//! DesktopDrawers --open <drawer-id>    -> OpenDrawer
//! ```

use crate::error::{Error, Result};

/// A parsed startup request. This is also the payload forwarded over IPC to an
/// already-running primary instance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Open (or focus) the drawer manager.
    Manager,
    /// Open (or focus) a specific drawer by its UUID.
    OpenDrawer(String),
}

impl Command {
    /// Parse from an argument iterator that does *not* include argv[0].
    pub fn parse<I, S>(args: I) -> Result<Command>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut iter = args.into_iter();
        let Some(first) = iter.next() else {
            return Ok(Command::Manager);
        };

        match first.as_ref() {
            "--manage" => Ok(Command::Manager),
            "--open" => {
                let id = iter
                    .next()
                    .ok_or_else(|| Error::BadCommandLine("--open requires a drawer id".into()))?;
                let id = id.as_ref().trim();
                if id.is_empty() {
                    return Err(Error::BadCommandLine("--open drawer id was empty".into()));
                }
                Ok(Command::OpenDrawer(id.to_string()))
            }
            other => Err(Error::BadCommandLine(format!("unknown argument: {other}"))),
        }
    }

    /// Parse from the real process command line (skips argv[0]).
    pub fn from_env() -> Result<Command> {
        Command::parse(std::env::args().skip(1))
    }

    /// Serialize to the JSON envelope used on the IPC pipe.
    pub fn to_ipc_json(&self) -> String {
        match self {
            Command::Manager => r#"{"command":"open_manager"}"#.to_string(),
            Command::OpenDrawer(id) => {
                // id is a UUID; still escape defensively.
                let escaped = id.replace('\\', "\\\\").replace('"', "\\\"");
                format!(r#"{{"command":"open_drawer","drawer_id":"{escaped}"}}"#)
            }
        }
    }

    /// Parse an IPC JSON envelope back into a `Command`.
    pub fn from_ipc_json(text: &str) -> Result<Command> {
        #[derive(serde::Deserialize)]
        struct Envelope {
            command: String,
            #[serde(default)]
            drawer_id: Option<String>,
        }
        let env: Envelope = serde_json::from_str(text.trim())?;
        match env.command.as_str() {
            "open_manager" => Ok(Command::Manager),
            "open_drawer" => env
                .drawer_id
                .filter(|s| !s.is_empty())
                .map(Command::OpenDrawer)
                .ok_or_else(|| Error::BadCommandLine("open_drawer without drawer_id".into())),
            other => Err(Error::BadCommandLine(format!("unknown IPC command: {other}"))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_args_is_manager() {
        assert_eq!(Command::parse(Vec::<String>::new()).unwrap(), Command::Manager);
    }

    #[test]
    fn manage_flag() {
        assert_eq!(Command::parse(["--manage"]).unwrap(), Command::Manager);
    }

    #[test]
    fn open_flag() {
        assert_eq!(
            Command::parse(["--open", "abc-123"]).unwrap(),
            Command::OpenDrawer("abc-123".into())
        );
    }

    #[test]
    fn open_without_id_errors() {
        assert!(Command::parse(["--open"]).is_err());
    }

    #[test]
    fn unknown_arg_errors() {
        assert!(Command::parse(["--frobnicate"]).is_err());
    }

    #[test]
    fn ipc_roundtrip_manager() {
        let c = Command::Manager;
        assert_eq!(Command::from_ipc_json(&c.to_ipc_json()).unwrap(), c);
    }

    #[test]
    fn ipc_roundtrip_open() {
        let c = Command::OpenDrawer("7dcf239e-5d4d-45af-8293-a354ee90d002".into());
        assert_eq!(Command::from_ipc_json(&c.to_ipc_json()).unwrap(), c);
    }
}
