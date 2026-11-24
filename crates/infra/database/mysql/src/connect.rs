//! The driver's side: connecting (a runtime of the session's own, TLS by `sslmode`, falling
//! back as the mode allows) and the sessions it opens; a sign-in refused.

use std::sync::Arc;
use std::time::Duration;

use database::fields::ConnectionSpec;
use database::mysql::{MySql as MyDialect, ServerVersion};
use database::tls::SSLROOTCERT;
use database::{
    Capabilities, DbError, Dialect, Driver, Kind, Language, Secret, Session, SslMode, Target,
};
use mysql_async::prelude::Queryable;
use mysql_async::{Conn, Opts, OptsBuilder};

use crate::tls::Attempt;
use crate::{
    CONNECT_TIMEOUT, CONNECT_TIMEOUT_OPTION, Killer, MySession, MySqlDriver, capabilities,
    connection, console, error, tls,
};

impl Driver for MySqlDriver {
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
        Some(Arc::new(MyDialect::default()))
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
        let password = secret.expose();
        let base = OptsBuilder::default()
            .ip_or_hostname(if target.host.is_empty() { "localhost" } else { &target.host })
            .tcp_port(target.port.unwrap_or(3306))
            .user(Some(target.user.clone()))
            .pass((!password.is_empty()).then(|| password.to_owned()))
            .db_name((!target.database.is_empty()).then(|| target.database.clone()))
            // An update's count is the rows it found (the grid's "exactly one row").
            .client_found_rows(true)
            .prefer_socket(false)
            .stmt_cache_size(0)
            .connect_attribute("program_name", "editor");
        let has_root = tls::file_option(target, SSLROOTCERT).is_some();
        let mut last = None;
        let mut opened = None;
        for attempt in tls::attempts(mode, has_root) {
            let opts: Opts = match attempt {
                Attempt::Plain => base.clone().ssl_opts(None::<mysql_async::SslOpts>).into(),
                Attempt::TlsOrPlain(check) | Attempt::Tls(check) => {
                    base.clone().ssl_opts(tls::ssl_opts(target, check)?).into()
                }
            };
            let connected = runtime.block_on(async {
                match tokio::time::timeout(timeout, Conn::new(opts.clone())).await {
                    Ok(result) => result,
                    Err(_) => Err(mysql_async::Error::Io(mysql_async::IoError::Io(
                        std::io::Error::new(std::io::ErrorKind::TimedOut, "connecting timed out"),
                    ))),
                }
            });
            match connected {
                Ok(conn) => {
                    opened = Some((conn, opts));
                    break;
                }
                // `prefer`: a server without TLS (or whose TLS can't be agreed on: an old
                // server's ciphers) is connected to plainly, as libpq does.
                Err(
                    mysql_async::Error::Driver(mysql_async::DriverError::NoClientSslFlagFromServer)
                    | mysql_async::Error::Io(_),
                ) if matches!(attempt, Attempt::TlsOrPlain(_)) => {
                    let plain: Opts = base.ssl_opts(None::<mysql_async::SslOpts>).into();
                    match runtime.block_on(async {
                        tokio::time::timeout(timeout, Conn::new(plain.clone())).await
                    }) {
                        Ok(Ok(conn)) => {
                            opened = Some((conn, plain));
                            break;
                        }
                        Ok(Err(e)) => last = Some(refused(e)),
                        Err(_) => last = Some(DbError::Connect("connecting timed out".into())),
                    }
                    break;
                }
                // `allow`: plain refused (the server wants TLS: 3159), so TLS next.
                Err(mysql_async::Error::Server(e)) if e.code == SECURE_TRANSPORT_REQUIRED => {
                    last = Some(refused(mysql_async::Error::Server(e)));
                }
                Err(e) => {
                    last = Some(refused(e));
                    break;
                }
            }
        }
        let Some((mut conn, opts)) = opened else {
            return Err(last.unwrap_or_else(|| DbError::Connect("no attempt".into())));
        };
        let version_text: String = runtime
            .block_on(conn.query_first::<String, _>("SELECT VERSION()"))
            .map_err(error)?
            .unwrap_or_default();
        let version = ServerVersion::parse(&version_text).unwrap_or_default();
        let server = format!("{} {}", version.engine(), version.text());
        // A read-only source writes nothing, whatever the app checks before.
        if target.read_only() {
            runtime.block_on(conn.query_drop(console::READ_ONLY_SESSION)).map_err(error)?;
        }
        let killer = Killer { opts, id: conn.id() };
        Ok(Box::new(MySession {
            conn: Some(conn),
            runtime,
            killer,
            version,
            server,
            in_tx: false,
            cursors: Vec::new(),
        }))
    }
}

/// The server refuses a plain connection (`require_secure_transport`).
const SECURE_TRANSPORT_REQUIRED: u16 = 3159;

/// A connection refused, as the domain says it.
fn refused(e: mysql_async::Error) -> DbError {
    error(e)
}
