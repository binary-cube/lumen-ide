//! Moving through the wizard: the form read into a connection (or why it can't be), the
//! steps' buttons (next, back, test, sign in, save), the connector that browses with the
//! password kept.

use super::*;

/// Editing with the password left empty: the kept secret, read from where it's kept each
/// time it connects (never held by the form).
struct KeptSecret {
    saved: Saved,
    /// Where it was kept (its identity, its store).
    original: Saved,
    accepted: Option<String>,
    connectors: Connectors,
    stores: Stores,
}

impl Connector for KeptSecret {
    fn connect(&self) -> Result<Box<dyn remote::application::RemoteFs>, RemoteError> {
        let secret = (self.stores)(&self.original).get(&self.original.connection.id);
        (self.connectors)(&self.saved, secret, self.accepted.clone()).connect()
    }
}

impl ConnectView {
    /// The connection as typed (the server's folder: `root`), with its secret — checked
    /// first: the server, the port, the user, the key.
    pub(crate) fn connection(&self, root: &str) -> Result<(Saved, Option<Secret>), Invalid> {
        let host = self.value(Field::Host);
        let user = self.value(Field::User);
        if host.is_empty() {
            return Err(Invalid::at(Field::Host, "remote.form_incomplete"));
        }
        if host.contains(char::is_whitespace) || host.contains('/') {
            return Err(Invalid::at(Field::Host, "remote.invalid_host"));
        }
        let port = match self.value(Field::Port) {
            typed if typed.is_empty() => self.protocol.default_port(),
            typed => match typed.parse::<u16>() {
                Ok(port) if port > 0 => port,
                _ => return Err(Invalid::at(Field::Port, "remote.invalid_port")),
            },
        };
        if user.is_empty() {
            return Err(Invalid::at(Field::User, "remote.form_incomplete"));
        }
        let auth = match (self.auth, self.protocol != Protocol::Sftp) {
            (_, true) | (AuthKind::Password, _) => Auth::Password,
            (AuthKind::Key, _) if self.value(Field::Key).is_empty() => {
                return Err(Invalid::at(Field::Key, "remote.invalid_key"));
            }
            (AuthKind::Key, _) => Auth::Key { path: PathBuf::from(self.value(Field::Key)) },
            (AuthKind::Agent, _) => Auth::Agent,
        };
        let name = match self.value(Field::Label) {
            label if label.is_empty() => self.value(Field::Name),
            label => label,
        };
        let mut excluded = vec![".git".to_owned(), kernel::brand::PROJECT_DIR.to_owned()];
        excluded.extend(self.excluded.iter().cloned());
        // Editing: what isn't on the form stays as it was (its retries, the certificates
        // trusted, its local copy).
        let original = self.editing.as_ref();
        let mut trusted: Vec<String> = original.map(|o| o.trusted.clone()).unwrap_or_default();
        trusted.extend(
            self.accepted.iter().filter(|a| !trusted.contains(a)).cloned().collect::<Vec<_>>(),
        );
        let connection = Connection {
            id: self.connection_id.clone(),
            name: if name.is_empty() { host.clone() } else { name },
            protocol: self.protocol,
            host,
            port,
            user,
            auth,
            root: root.to_owned(),
            mode: self.mode,
            excluded,
            saved_in: self.saved_in,
            secrets_in: self.secrets_in,
            retry: original.and_then(|o| o.connection.retry),
            uploads: self.uploads,
            downloads: self.downloads,
            moves: self.moves,
            auto_upload: self.auto_upload,
            write_as: if self.owners_changeable() {
                self.write_as.clone()
            } else {
                WriteAs::Server
            },
        };
        let secret = Secret::new(self.input(Field::Secret).text());
        // The kept one, as read: not a new one (it stays where it is, read when connecting).
        let secret =
            if self.kept_secret.as_ref() == Some(&secret) { Secret::default() } else { secret };
        let local = match original {
            Some(o) => o.local.clone(),
            None => PathBuf::from(self.value(Field::Local)),
        };
        let saved = Saved { connection, local, trusted };
        Ok((saved, (!secret.is_empty()).then_some(secret)))
    }

    /// The whole connection, ready to download (step 3) or to keep (editing).
    pub(crate) fn saved(&self) -> Result<(Saved, Option<Secret>), Invalid> {
        let (saved, secret) = self.connection(&self.root)?;
        if !self.root.starts_with('/') {
            return Err(Invalid { field: None, why: t("remote.invalid_path") });
        }
        if self.value(Field::Name).is_empty() {
            return Err(Invalid::at(Field::Name, "remote.form_incomplete"));
        }
        if saved.local.as_os_str().is_empty() {
            return Err(Invalid::at(Field::Local, "remote.form_incomplete"));
        }
        Ok((saved, secret))
    }

    /// What's wrong said in the footer, its field outlined.
    pub(super) fn reject(&mut self, invalid: Invalid) {
        self.status = Some((invalid.why, Tone::Danger));
        self.invalid = invalid.field;
        if let Some(field) = invalid.field.filter(|f| self.fields().contains(f)) {
            self.focus(field);
            self.on_list = false;
        }
    }

    /// A connector for browsing (paths from the server's root). Editing with the password
    /// left empty: the kept one, read when it connects (on the browsing thread).
    fn browsing_connector(&self) -> Result<Arc<dyn Connector>, Invalid> {
        let (saved, secret) = self.connection("/")?;
        let accepted = self.accepted.clone();
        Ok(match (&self.editing, secret) {
            (Some(original), None) if saved.connection.auth != Auth::Agent => {
                Arc::new(KeptSecret {
                    saved,
                    original: original.clone(),
                    accepted,
                    connectors: self.connectors,
                    stores: self.stores,
                })
            }
            (_, secret) => (self.connectors)(&saved, secret, accepted),
        })
    }

    /// Enter: the step's main action (editing, on the form and the options: Save).
    pub(super) fn next(&mut self) {
        match self.step {
            Step::Connection | Step::Contents if self.editing.is_some() => self.save(),
            Step::Connection => self.browse(),
            // The typed path: gone there.
            Step::Folder if !self.on_list => {
                let dir = self.value(Field::Path);
                let dir = if dir.starts_with('/') { dir } else { join(&self.current, &dir) };
                self.go(dir);
            }
            Step::Folder => self.choose(),
            Step::Contents => self.connect(),
        }
    }

    /// Step 1 → 2: connects (a session kept for browsing) and lands where the user does
    /// (editing: in the saved folder).
    pub(super) fn browse(&mut self) {
        self.sign_in();
        self.testing = false;
    }

    /// Editing: signs in with what's typed, and says how it went (it stays on the step).
    pub(super) fn test(&mut self) {
        self.sign_in();
        self.testing = self.waiting > 0;
    }

    /// Connects with what's typed (a session kept for browsing), asking where the user lands.
    fn sign_in(&mut self) {
        // The fingerprint shown: going on trusts it.
        if let Some(fingerprint) = self.unknown.take() {
            self.accepted = Some(fingerprint);
        }
        let connector = match self.browsing_connector() {
            Ok(c) => c,
            Err(invalid) => {
                self.reject(invalid);
                return;
            }
        };
        self.invalid = None;
        self.open_browser(connector);
        self.ask(Request::Home);
        self.status = Some((t("remote.testing"), Tone::Neutral));
    }

    /// Editing: keeps the connection as typed (its secret with it, a new one replacing the
    /// kept), in the background; then the project reconnects with it if it's open.
    pub(super) fn save(&mut self) {
        let Some(original) = self.editing.clone() else { return };
        if self.busy() {
            return;
        }
        let (saved, secret) = match self.saved() {
            Ok(s) => s,
            Err(invalid) => {
                self.reject(invalid);
                return;
            }
        };
        self.invalid = None;
        let stores = self.stores;
        let waker = self.waker.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        kernel::work::run("remote-save", move || {
            let outcome = config::update(&original, &saved, secret.as_ref(), &|s| stores(s));
            let _ = tx.send(outcome.map(|()| (saved, secret)));
            if let Some(wake) = &waker {
                wake();
            }
        });
        self.saving = Some(rx);
        self.status = Some((t("remote.saving"), Tone::Neutral));
    }

    /// The edited connection kept (or why not): the project reconnects with it, the form
    /// closes. `true` if it ended.
    pub(super) fn saved_now(&mut self) -> bool {
        let Some(outcome) = self.saving.as_ref().and_then(|rx| rx.try_recv().ok()) else {
            return false;
        };
        self.saving = None;
        match outcome {
            Ok((saved, secret)) => {
                let key = if session::reconnect_later(saved, secret) {
                    "remote.saved_reconnecting"
                } else {
                    "remote.saved"
                };
                self.status = Some((t(key), Tone::Success));
                self.action = Some(ViewerAction::Close);
            }
            Err(config::Unsaved::Secret(why)) => {
                self.status =
                    Some((tr!("remote.error_secret", reason = why.as_str()), Tone::Danger));
            }
            Err(config::Unsaved::Save(why)) => {
                self.status = Some((tr!("remote.error_save", reason = why.as_str()), Tone::Danger));
            }
        }
        true
    }

    pub(super) fn back(&mut self) {
        self.step = match self.step {
            Step::Connection | Step::Folder => Step::Connection,
            Step::Contents => Step::Folder,
        };
        self.status = None;
        self.scroll = 0.0;
        self.on_list = self.step != Step::Connection;
        self.highlight = None;
        self.suggestions.clear();
        self.local_folders = None;
        if self.step == Step::Connection {
            self.browser = None;
            self.server_folders.clear();
            self.focus(Field::Host);
        }
    }
}
