//! `spawn_sandboxed`: a plugin run in a child process (FW-17,
//! `docs/plugin-sandbox-adr.md`).
//!
//! The child is the `vieww-plugin-host` binary of this crate. It loads the
//! plugin with the ordinary in-process loader ([`PluginRegistry::load`],
//! unchanged — every promise `docs/plugin-abi.md` makes holds inside the
//! child) and serves calls over its stdin/stdout. The parent never links the
//! plugin: a plugin that aborts, segfaults, spins forever or allocates
//! without bound takes the child down, not the host, and the next call says
//! so ([`SandboxError::Died`], [`SandboxError::TimedOut`]).
//!
//! **Fault and resource isolation, not a security sandbox.** A hostile
//! native process can lie over a pipe and do anything its user can; OS-level
//! confinement is a separate, per-platform layer the ADR leaves for its own
//! decision.
//!
//! # The wire
//!
//! Frames of `u32` little-endian length + UTF-8 body, the body's fields
//! separated by U+001F (unit separator) and list items by U+001E (record
//! separator) — characters a command id, title, name or path never contains,
//! so no escaping is needed. The ADR sketched JSON; this crate has no JSON
//! (its only dependency is `libloading`) and the protocol is five messages,
//! so the framing is std-only instead and the ADR's tail records the change.
//!
//! | parent → child     | child → parent                                     |
//! |--------------------|----------------------------------------------------|
//! | `load␟path`        | `loaded␟name␟version␟id␞title␟id␞title…` / `err␟…` |
//! | `invoke␟id`        | `ok` / `err␟kind␟detail`                           |
//! | `shutdown`         | *(exit)*                                           |

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::Duration;

use crate::abi::PluginCommand;
use crate::registry::{PluginError, PluginRegistry};

/// Separates a message's fields.
pub const FIELD: char = '\u{1f}';
/// Separates a list's items (a command's id from its title).
pub const ITEM: char = '\u{1e}';

/// What a sandboxed plugin is allowed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxPolicy {
    /// Kill the child after this long without an answer to a call.
    pub invoke_timeout: Duration,
    /// Cap on the child's address space, in bytes, where the OS offers one
    /// (`RLIMIT_AS` on Linux and macOS, applied by the child to itself before
    /// the plugin loads). Not enforced on Windows yet — a job object is the
    /// mechanism, and it is noted untested there.
    pub memory_limit: Option<usize>,
    /// A reply larger than this kills the child: a plugin answering a
    /// five-message protocol with megabytes is broken, and the parent stops
    /// reading before it allocates.
    pub max_reply_bytes: usize,
    /// The `vieww-plugin-host` binary. `None` looks for it (see
    /// [`find_host_binary`]).
    pub host_binary: Option<PathBuf>,
}

impl Default for SandboxPolicy {
    fn default() -> Self {
        Self {
            invoke_timeout: Duration::from_secs(5),
            memory_limit: None,
            max_reply_bytes: 1 << 20,
            host_binary: None,
        }
    }
}

/// Why a sandboxed call failed.
#[derive(Debug)]
pub enum SandboxError {
    /// The host binary could not be found or started.
    Spawn(String),
    /// The plugin, inside the child, answered with the ordinary error the
    /// in-process loader would have returned — the same variants, so a
    /// caller handles a sandboxed plugin's refusals exactly as an in-process
    /// one's. The library's own `libloading` error is carried as text.
    Plugin(PluginError),
    /// The child's loader failed in a way that has no `PluginError` (a
    /// library that would not open, by its text).
    Load(String),
    /// The child exited (abort, a segfault, `exit`) without answering.
    Died(String),
    /// The child did not answer within [`SandboxPolicy::invoke_timeout`];
    /// it has been killed.
    TimedOut(String),
    /// The child broke the protocol (an oversized or malformed reply); it
    /// has been killed.
    Protocol(String),
}

impl std::fmt::Display for SandboxError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Spawn(why) => write!(f, "could not start the plugin host: {why}"),
            Self::Plugin(error) => write!(f, "{error}"),
            Self::Load(why) => write!(f, "could not load the plugin: {why}"),
            Self::Died(call) => write!(f, "the sandboxed plugin's process died during {call}"),
            Self::TimedOut(call) => write!(f, "the sandboxed plugin did not answer {call} in time"),
            Self::Protocol(why) => write!(f, "the sandboxed plugin broke the protocol: {why}"),
        }
    }
}

impl std::error::Error for SandboxError {}

/// Write one frame.
///
/// # Errors
///
/// The pipe's error.
pub fn write_frame(out: &mut impl Write, body: &str) -> std::io::Result<()> {
    let len = u32::try_from(body.len())
        .map_err(|_| std::io::Error::other("a frame longer than 4 GiB"))?;
    out.write_all(&len.to_le_bytes())?;
    out.write_all(body.as_bytes())?;
    out.flush()
}

/// Read one frame, refusing one longer than `max` before reading its body.
///
/// # Errors
///
/// End of stream (`UnexpectedEof`), an oversized frame (`InvalidData`), or
/// a body that is not UTF-8.
pub fn read_frame(input: &mut impl Read, max: usize) -> std::io::Result<String> {
    let mut len = [0u8; 4];
    input.read_exact(&mut len)?;
    let len = u32::from_le_bytes(len) as usize;
    if len > max {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("a {len}-byte reply, over the {max}-byte cap"),
        ));
    }
    let mut body = vec![0u8; len];
    input.read_exact(&mut body)?;
    String::from_utf8(body).map_err(|_| {
        std::io::Error::new(std::io::ErrorKind::InvalidData, "a reply that is not UTF-8")
    })
}

/// Where `vieww-plugin-host` is: `VIEWW_PLUGIN_HOST`, else beside the
/// running executable, else one directory up (a test binary in
/// `target/<profile>/deps` beside its crate's binaries in `target/<profile>`).
#[must_use]
pub fn find_host_binary() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("VIEWW_PLUGIN_HOST") {
        return Some(PathBuf::from(path));
    }
    let name = format!("vieww-plugin-host{}", std::env::consts::EXE_SUFFIX);
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    [dir.join(&name), dir.parent()?.join(&name)]
        .into_iter()
        .find(|candidate| candidate.is_file())
}

/// A plugin loaded in a child process. Dropping it shuts the child down.
pub struct SandboxedPlugin {
    child: Child,
    stdin: Option<ChildStdin>,
    replies: Receiver<std::io::Result<String>>,
    policy: SandboxPolicy,
    path: PathBuf,
    name: String,
    version: String,
    commands: Vec<PluginCommand>,
    dead: Option<String>,
}

impl std::fmt::Debug for SandboxedPlugin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SandboxedPlugin")
            .field("path", &self.path)
            .field("name", &self.name)
            .field("version", &self.version)
            .field("dead", &self.dead)
            .finish_non_exhaustive()
    }
}

impl PluginRegistry {
    /// Load `path` in a supervisor child process instead of this one — see
    /// the [`sandbox`](crate::sandbox) module and the ADR.
    ///
    /// Opt-in per plugin: trusted plugins keep [`load`](Self::load)'s
    /// zero-copy path. A sandboxed plugin is not added to this registry's
    /// list; the returned handle is the plugin.
    ///
    /// # Errors
    ///
    /// [`SandboxError::Spawn`] without a host binary; otherwise whatever the
    /// child's load answered, or how it failed to.
    pub fn spawn_sandboxed(
        &self,
        path: impl AsRef<Path>,
        policy: SandboxPolicy,
    ) -> Result<SandboxedPlugin, SandboxError> {
        SandboxedPlugin::spawn(path.as_ref(), policy)
    }
}

impl SandboxedPlugin {
    fn spawn(path: &Path, policy: SandboxPolicy) -> Result<Self, SandboxError> {
        let host = policy
            .host_binary
            .clone()
            .or_else(find_host_binary)
            .ok_or_else(|| {
                SandboxError::Spawn(
                    "no vieww-plugin-host binary (set VIEWW_PLUGIN_HOST, or ship it beside the executable)"
                        .to_owned(),
                )
            })?;
        let mut command = Command::new(&host);
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            // The plugin's logging reaches the operator's console, as an
            // in-process plugin's does.
            .stderr(Stdio::inherit());
        if let Some(limit) = policy.memory_limit {
            command.env("VIEWW_PLUGIN_MEMORY_LIMIT", limit.to_string());
        }
        let mut child = command
            .spawn()
            .map_err(|error| SandboxError::Spawn(format!("{}: {error}", host.display())))?;
        let stdin = child.stdin.take();
        let mut stdout = child
            .stdout
            .take()
            .ok_or_else(|| SandboxError::Spawn("no pipe to the child".to_owned()))?;

        // The reader thread owns the child's stdout, so a parent waiting on
        // an answer waits on a channel with a deadline rather than on a read
        // nothing can interrupt. It ends with the pipe.
        let (send, replies) = mpsc::channel();
        let cap = policy.max_reply_bytes;
        std::thread::Builder::new()
            .name("vieww-plugin-sandbox-reader".to_owned())
            .spawn(move || loop {
                let frame = read_frame(&mut stdout, cap);
                let end = frame.is_err();
                if send.send(frame).is_err() || end {
                    break;
                }
            })
            .map_err(|error| SandboxError::Spawn(error.to_string()))?;

        let mut plugin = Self {
            child,
            stdin,
            replies,
            policy,
            path: path.to_path_buf(),
            name: String::new(),
            version: String::new(),
            commands: Vec::new(),
            dead: None,
        };
        let reply = plugin.call(&format!("load{FIELD}{}", path.display()), "load")?;
        let mut fields = reply.split(FIELD);
        match fields.next() {
            Some("loaded") => {
                plugin.name = fields.next().unwrap_or_default().to_owned();
                plugin.version = fields.next().unwrap_or_default().to_owned();
                plugin.commands = fields
                    .filter(|item| !item.is_empty())
                    .map(|item| {
                        let (id, title) = item.split_once(ITEM).unwrap_or((item, item));
                        PluginCommand::new(id, title)
                    })
                    .collect();
                Ok(plugin)
            }
            Some("err") => Err(decode_error(fields)),
            _ => {
                plugin.kill("load");
                Err(SandboxError::Protocol(format!(
                    "{reply:?} is not an answer to load"
                )))
            }
        }
    }

    /// Send one request and wait for its answer under the policy.
    fn call(&mut self, request: &str, what: &str) -> Result<String, SandboxError> {
        if let Some(why) = &self.dead {
            return Err(SandboxError::Died(why.clone()));
        }
        let Some(stdin) = self.stdin.as_mut() else {
            return Err(SandboxError::Died(what.to_owned()));
        };
        if write_frame(stdin, request).is_err() {
            return Err(self.died(what));
        }
        match self.replies.recv_timeout(self.policy.invoke_timeout) {
            Ok(Ok(reply)) => Ok(reply),
            Ok(Err(error)) if error.kind() == std::io::ErrorKind::InvalidData => {
                self.kill(what);
                Err(SandboxError::Protocol(error.to_string()))
            }
            Ok(Err(_)) | Err(RecvTimeoutError::Disconnected) => Err(self.died(what)),
            Err(RecvTimeoutError::Timeout) => {
                self.kill(what);
                Err(SandboxError::TimedOut(what.to_owned()))
            }
        }
    }

    /// The child is gone: reap it and remember why, for every later call.
    fn died(&mut self, what: &str) -> SandboxError {
        let status = self
            .child
            .wait()
            .map_or_else(|_| "an unknown status".to_owned(), |s| s.to_string());
        let why = format!("{what} ({status})");
        self.dead = Some(why.clone());
        self.stdin = None;
        SandboxError::Died(why)
    }

    fn kill(&mut self, what: &str) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        self.stdin = None;
        self.dead = Some(format!("{what} (killed by the sandbox)"));
    }

    /// The library this was loaded from.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The plugin's own name, as its `name()` answered in the child.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The plugin's own version string.
    #[must_use]
    pub fn version(&self) -> &str {
        &self.version
    }

    /// The commands the plugin offered after `init`, as the in-process
    /// [`LoadedPlugin::commands`](crate::LoadedPlugin::commands) would list them.
    #[must_use]
    pub fn commands(&self) -> &[PluginCommand] {
        &self.commands
    }

    /// Whether the child is still serving.
    #[must_use]
    pub const fn is_alive(&self) -> bool {
        self.dead.is_none()
    }

    /// Run the command named `id` in the child.
    ///
    /// # Errors
    ///
    /// [`SandboxError::Plugin`] with the same [`PluginError`] the in-process
    /// path returns for an unknown id, a failed handler or a caught panic;
    /// [`SandboxError::Died`] / [`SandboxError::TimedOut`] /
    /// [`SandboxError::Protocol`] when the child itself failed — after which
    /// every call answers `Died`, and a fresh
    /// [`spawn_sandboxed`](PluginRegistry::spawn_sandboxed) is the recovery.
    pub fn invoke(&mut self, id: &str) -> Result<(), SandboxError> {
        if !self.commands.iter().any(|command| command.id == id) && self.dead.is_none() {
            return Err(SandboxError::Plugin(PluginError::UnknownCommand(
                id.to_owned(),
            )));
        }
        let what = format!("invoke({id:?})");
        let reply = self.call(&format!("invoke{FIELD}{id}"), &what)?;
        let mut fields = reply.split(FIELD);
        match fields.next() {
            Some("ok") => Ok(()),
            Some("err") => Err(decode_error(fields)),
            _ => {
                self.kill(&what);
                Err(SandboxError::Protocol(format!(
                    "{reply:?} is not an answer to invoke"
                )))
            }
        }
    }
}

impl Drop for SandboxedPlugin {
    fn drop(&mut self) {
        if self.dead.is_some() {
            return;
        }
        if let Some(stdin) = self.stdin.as_mut() {
            let _ = write_frame(stdin, "shutdown");
        }
        self.stdin = None;
        // A child that does not exit on `shutdown` is not waited on forever.
        let deadline = std::time::Instant::now() + self.policy.invoke_timeout;
        loop {
            match self.child.try_wait() {
                Ok(Some(_)) => return,
                Ok(None) if std::time::Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(5));
                }
                _ => {
                    let _ = self.child.kill();
                    let _ = self.child.wait();
                    return;
                }
            }
        }
    }
}

/// `err␟kind␟detail` back into an error.
fn decode_error<'a>(mut fields: impl Iterator<Item = &'a str>) -> SandboxError {
    let kind = fields.next().unwrap_or_default();
    let detail = fields.next().unwrap_or_default().to_owned();
    let plugin = |error| SandboxError::Plugin(error);
    match kind {
        "unknown" => plugin(PluginError::UnknownCommand(detail)),
        "failed" => plugin(PluginError::InvokeFailed(detail)),
        "panicked" => plugin(PluginError::Panicked(detail)),
        "init" => plugin(PluginError::InitFailed),
        "null" => plugin(PluginError::NullVTable),
        "version" => {
            let mut nums = detail.split('.').map(|n| n.parse::<u32>().unwrap_or(0));
            let mut next = || nums.next().unwrap_or(0);
            let plugin_v = crate::AbiVersion {
                major: next(),
                minor: next(),
            };
            let host_v = crate::AbiVersion {
                major: next(),
                minor: next(),
            };
            plugin(PluginError::IncompatibleVersion {
                plugin: plugin_v,
                host: host_v,
            })
        }
        _ => SandboxError::Load(detail),
    }
}

/// `PluginError` as the child sends it: `kind␟detail`.
#[must_use]
pub fn encode_error(error: &PluginError) -> String {
    let (kind, detail) = match error {
        PluginError::UnknownCommand(id) => ("unknown", id.clone()),
        PluginError::InvokeFailed(id) => ("failed", id.clone()),
        PluginError::Panicked(call) => ("panicked", call.clone()),
        PluginError::InitFailed => ("init", String::new()),
        PluginError::NullVTable => ("null", String::new()),
        PluginError::IncompatibleVersion { plugin, host } => (
            "version",
            format!(
                "{}.{}.{}.{}",
                plugin.major, plugin.minor, host.major, host.minor
            ),
        ),
        PluginError::Open(error) | PluginError::MissingEntrySymbol(error) => {
            ("load", error.to_string())
        }
    };
    format!(
        "err{FIELD}{kind}{FIELD}{}",
        detail.replace([FIELD, ITEM], " ")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_round_trip_and_an_oversized_one_is_refused_unread() {
        let mut wire = Vec::new();
        write_frame(&mut wire, "invoke\u{1f}greet").unwrap();
        let mut input = wire.as_slice();
        assert_eq!(read_frame(&mut input, 64).unwrap(), "invoke\u{1f}greet");
        let mut wire = Vec::new();
        write_frame(&mut wire, &"x".repeat(100)).unwrap();
        let error = read_frame(&mut wire.as_slice(), 10).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
    }

    #[test]
    fn errors_cross_the_wire_as_the_same_plugin_errors() {
        for error in [
            PluginError::UnknownCommand("x".into()),
            PluginError::InvokeFailed("y".into()),
            PluginError::Panicked("invoke(\"z\")".into()),
            PluginError::InitFailed,
        ] {
            let wire = encode_error(&error);
            let back = decode_error(wire.split(FIELD).skip(1));
            assert_eq!(
                format!("{back}"),
                format!("{error}"),
                "{wire:?} did not round-trip"
            );
        }
    }
}
