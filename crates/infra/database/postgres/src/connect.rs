//! The driver's side: connecting (TLS by `sslmode`, a runtime of the session's own, the
//! connection driven with its notices kept) and the sessions it opens.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use database::fields::ConnectionSpec;
use database::postgres::{self as pg, Postgres as PgDialect};
use database::tls::SSLROOTCERT;
use database::{
    Capabilities, DbError, Dialect, Driver, Kind, Language, Notice, Secret, Session, SslMode,
    Target,
};
use futures_util::StreamExt;
use tokio_postgres::{AsyncMessage, Client, NoTls};

use crate::tls::Attempt;
use crate::{
    APPLICATION_NAME, CONNECT_TIMEOUT, CONNECT_TIMEOUT_OPTION, PgSession, Postgres, Transport,
    capabilities, connection, console, error, profile, refused, tls,
};

impl Driver for Postgres {
    fn kind(&self) -> Kind {
        Kind::Relational
    }

    fn language(&self) -> Language {
        Language::Sql
    }

    fn capabilities(&self) -> Capabilities {
        capabilities()
    }

    /// Before it connects: what every supported version takes (the session gives its own).
    fn dialect(&self) -> Option<Arc<dyn Dialect>> {
        Some(Arc::new(PgDialect::default()))
    }

    fn connection(&self) -> ConnectionSpec {
        connection()
    }

    fn connect(&self, target: &Target, secret: &Secret) -> Result<Box<dyn Session>, DbError> {
        let mode = SslMode::of(target).map_err(DbError::Connect)?;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| DbError::Connect(e.to_string()))?;
        let timeout = target
            .options
            .get(CONNECT_TIMEOUT_OPTION)
            .and_then(|t| t.trim().parse::<u64>().ok())
            .filter(|t| *t > 0)
            .map_or(CONNECT_TIMEOUT, Duration::from_secs);
        let mut config = tokio_postgres::Config::new();
        config
            .host(if target.host.is_empty() { "localhost" } else { &target.host })
            .port(target.port.unwrap_or(5432))
            .user(&target.user)
            .password(secret.expose().as_bytes())
            .connect_timeout(timeout)
            .application_name(
                target.options.get(APPLICATION_NAME).map_or("editor", String::as_str),
            );
        if !target.database.is_empty() {
            config.dbname(&target.database);
        }
        let has_root = tls::file_option(target, SSLROOTCERT).is_some();
        let mut last = None;
        let mut opened = None;
        let notices = Notices::default();
        for attempt in tls::attempts(mode, has_root) {
            match connect_once(&runtime, &mut config, target, attempt, &notices) {
                Ok(done) => {
                    opened = Some(done);
                    break;
                }
                // The server unreachable: no other attempt would reach it.
                Err(e @ DbError::Connect(_)) => {
                    last = Some(e);
                    break;
                }
                // `allow`: plain refused (the server wants TLS), so TLS next.
                Err(e) => last = Some(e),
            }
        }
        let (client, transport) = match opened {
            Some(done) => done,
            None => return Err(last.unwrap_or_else(|| DbError::Connect("no attempt".into()))),
        };
        let show = |name: &str| {
            runtime
                .block_on(client.query_one(&format!("SHOW {name}"), &[]))
                .ok()
                .map(|row| row.get::<_, String>(0))
        };
        let version_name = show("server_version").unwrap_or_default();
        let version = show("server_version_num")
            .and_then(|v| pg::parse_version_num(&v))
            .unwrap_or(pg::OLDEST);
        let server = if version_name.is_empty() {
            "PostgreSQL".to_owned()
        } else {
            // `16.4 (Debian 16.4-1)`: only the number.
            format!("PostgreSQL {}", version_name.split_whitespace().next().unwrap_or_default())
        };
        // A read-only source writes nothing, whatever the app checks before.
        if target.read_only() {
            runtime.block_on(client.batch_execute(console::READ_ONLY_SESSION)).map_err(error)?;
        }
        let block_size = show("block_size")
            .and_then(|b| b.trim().parse::<u32>().ok())
            .filter(|b| *b > 0)
            .unwrap_or(profile::BLOCK_SIZE);
        let cancel = client.cancel_token();
        Ok(Box::new(PgSession {
            runtime,
            client,
            cancel,
            transport,
            server,
            version,
            cursors: Vec::new(),
            notices,
            in_tx: false,
            cursor: None,
            next_cursor: 1,
            block_size,
            statement_stats: None,
        }))
    }
}

/// One connection attempt: the client (its connection driven on the runtime) and how its
/// cancel requests go.
fn connect_once(
    runtime: &tokio::runtime::Runtime,
    config: &mut tokio_postgres::Config,
    target: &Target,
    attempt: Attempt,
    notices: &Notices,
) -> Result<(Client, Transport), DbError> {
    use tokio_postgres::config::SslMode as Wire;
    let (wire, check) = match attempt {
        Attempt::Plain => (Wire::Disable, None),
        Attempt::TlsOrPlain(check) => (Wire::Prefer, Some(check)),
        Attempt::Tls(check) => (Wire::Require, Some(check)),
    };
    config.ssl_mode(wire);
    let transport = match check {
        None => Transport::Plain,
        Some(check) => Transport::Tls(tokio_postgres_rustls::MakeRustlsConnect::new(
            tls::client_config(target, check)?,
        )),
    };
    let client = runtime.block_on(async {
        match &transport {
            Transport::Plain => {
                let (client, connection) = config.connect(NoTls).await.map_err(refused)?;
                tokio::spawn(drive(connection, Arc::clone(notices)));
                Ok::<_, DbError>(client)
            }
            Transport::Tls(tls) => {
                let (client, connection) = config.connect(tls.clone()).await.map_err(refused)?;
                tokio::spawn(drive(connection, Arc::clone(notices)));
                Ok(client)
            }
        }
    })?;
    Ok((client, transport))
}

/// The server's notices since they were last taken (the newest [`console::NOTICES`]).
pub(crate) type Notices = Arc<Mutex<VecDeque<Notice>>>;

/// Drives a connection, keeping the notices it brings (the server's other messages
/// dropped); ends with it.
async fn drive<S, T>(mut connection: tokio_postgres::Connection<S, T>, notices: Notices)
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
    T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let mut messages = futures_util::stream::poll_fn(move |cx| connection.poll_message(cx));
    while let Some(message) = messages.next().await {
        match message {
            Ok(AsyncMessage::Notice(notice)) => {
                let mut kept = notices.lock().unwrap_or_else(PoisonError::into_inner);
                if kept.len() == console::NOTICES {
                    kept.pop_front();
                }
                kept.push_back(console::notice(&notice));
            }
            Ok(_) => {}
            Err(_) => break,
        }
    }
}
