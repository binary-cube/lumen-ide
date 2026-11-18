//! How a server's process is started: a program with its pipes (a real one through the
//! `process` tools: its own process group, no console window on Windows, stopped with what
//! it started) — or listening on a local port the client connects to (`transport = "tcp"`)
//! —, or anything else speaking LSP on two streams (a test's server in memory).

use std::io::{self, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::spec::{Launch, Transport};

/// How long a server started on a port has to listen on it.
const CONNECT_WITHIN: Duration = Duration::from_secs(20);

/// A started server's streams, and what keeps it running (dropped: stopped).
pub struct Connection {
    /// Its stdin.
    pub input: Box<dyn Write + Send>,
    /// Its stdout.
    pub output: Box<dyn Read + Send>,
    /// Its stderr, if it has one.
    pub errors: Option<Box<dyn Read + Send>>,
    pub pid: Option<u32>,
    /// What stops it when dropped (its process).
    pub keep: Option<Box<dyn std::any::Any + Send>>,
}

impl std::fmt::Debug for Connection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Connection").field("pid", &self.pid).finish_non_exhaustive()
    }
}

/// Starts a server (again, after a crash).
pub trait Launcher: Send {
    fn launch(&mut self) -> io::Result<Connection>;
}

/// A server that is a program.
#[derive(Debug, Clone)]
pub struct ProcessLauncher {
    pub launch: Launch,
    /// The folder it runs in (its root).
    pub dir: PathBuf,
}

impl Launcher for ProcessLauncher {
    fn launch(&mut self) -> io::Result<Connection> {
        // A port free now, for a server that listens on one (named in its arguments).
        let port = match self.launch.transport {
            Transport::Stdio => None,
            Transport::Tcp => Some(free_port()?),
        };
        let args: Vec<std::ffi::OsString> = match port {
            Some(port) => self
                .launch
                .args
                .iter()
                .map(|a| a.to_string_lossy().replace("${port}", &port.to_string()).into())
                .collect(),
            None => self.launch.args.clone(),
        };
        let mut run = process::Run::new(&self.launch.program, &args).dir(&self.dir).owner("lsp");
        for (key, value) in &self.launch.env {
            run = run.env(key, value);
        }
        let mut started = process::start(&run).map_err(|e| io::Error::other(e.to_string()))?;
        let errors = started.stderr.take().map(|e| Box::new(e) as Box<dyn Read + Send>);
        let pid = Some(started.id());
        let (input, output): (Box<dyn Write + Send>, Box<dyn Read + Send>) = match port {
            None => {
                let input = started.stdin.take().ok_or_else(|| io::Error::other("no stdin"))?;
                let output = started.stdout.take().ok_or_else(|| io::Error::other("no stdout"))?;
                (Box::new(input), Box::new(output))
            }
            Some(port) => {
                // What it writes on its stdout isn't the protocol: read and dropped (a full
                // pipe would stop it).
                if let Some(mut out) = started.stdout.take() {
                    std::thread::Builder::new()
                        .name("lsp-stdout".into())
                        .spawn(move || io::copy(&mut out, &mut io::sink()).map(drop))?;
                }
                let stream = connect(port, &mut started)?;
                (Box::new(stream.try_clone()?), Box::new(stream))
            }
        };
        Ok(Connection { input, output, errors, pid, keep: Some(Box::new(started)) })
    }
}

/// A local port nobody listens on now.
fn free_port() -> io::Result<u16> {
    Ok(TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?.local_addr()?.port())
}

/// The server started on `port`, connected to once it listens (it may take a moment to
/// start); an error if it exits first or doesn't listen in time.
fn connect(port: u16, started: &mut process::Started) -> io::Result<TcpStream> {
    let address = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let deadline = Instant::now() + CONNECT_WITHIN;
    loop {
        match TcpStream::connect_timeout(&address, Duration::from_millis(250)) {
            Ok(stream) => {
                stream.set_nodelay(true)?;
                return Ok(stream);
            }
            Err(e) => {
                if let Some(code) = started.exited() {
                    return Err(io::Error::other(format!(
                        "it exited ({code:?}) before listening on port {port}"
                    )));
                }
                if Instant::now() >= deadline {
                    return Err(io::Error::other(format!("nothing listens on port {port}: {e}")));
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        }
    }
}

/// A server made by a function (a test's, in memory): each launch calls it again.
pub struct FnLauncher(pub Box<dyn FnMut() -> io::Result<Connection> + Send>);

impl std::fmt::Debug for FnLauncher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("FnLauncher")
    }
}

impl Launcher for FnLauncher {
    fn launch(&mut self) -> io::Result<Connection> {
        (self.0)()
    }
}
