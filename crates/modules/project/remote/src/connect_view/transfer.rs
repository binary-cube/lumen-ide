//! Downloading the chosen folder (on its own thread), its progress counted.

use super::*;

/// The download itself (a background thread): listed, planned, transferred in parallel,
/// then the manifest, the secret and the connection kept.
pub(super) fn download(
    saved: &Saved,
    secret: Option<remote::Secret>,
    connector: &dyn Connector,
    progress: &(AtomicUsize, AtomicUsize),
    cancel: &AtomicBool,
    wake: &(dyn Fn() + Sync),
) -> Downloaded {
    // The secret first: if it can't be kept, nothing is downloaded for nothing.
    if let Some(secret) = &secret {
        config::secret_store(saved)
            .set(&saved.connection.id, secret.expose())
            .map_err(Failure::Secret)?;
    }
    // On the server: nothing comes down now (files are brought when opened); the server
    // answered, the local folder is made, the connection kept.
    if saved.connection.mode == Mode::OnDemand {
        drop(connector.connect()?);
        std::fs::create_dir_all(config::data_dir(&saved.local))
            .map_err(|e| Failure::Save(e.to_string()))?;
        session::save_manifest(&saved.local, &Manifest::default());
        config::save(saved).map_err(|e| Failure::Save(e.to_string()))?;
        return Ok((saved.local.clone(), 0, 0));
    }
    let mut fs = connector.connect()?;
    let remote = list_remote(&saved.connection, fs.as_mut(), &|| cancel.load(Ordering::Relaxed))?;
    drop(fs);
    let local = session::local_copy(&saved.local);
    let mut manifest = Manifest::default();
    let here = remote::application::LocalFiles::list(&local);
    let changes = plan(&saved.connection, &manifest, &here, &remote, Direction::Pull, &|_| None);
    progress.1.store(changes.len(), Ordering::Relaxed);
    let run = Run {
        workers: crate::settings::defaults().limits.downloads,
        retry: saved.connection.retry.unwrap_or_default(),
        now: &session::now,
        on_transfer: &|_| {
            progress.0.fetch_add(1, Ordering::Relaxed);
            wake();
        },
        on_bytes: &|action, n| session::count(&session::OPENING.1, action, n),
        cancelled: &|| cancel.load(Ordering::Relaxed),
        parallel: &session::threads,
        pause: &std::thread::sleep,
    };
    let done = apply(&changes, connector, &local, &mut manifest, &run);
    let failed = done.iter().filter(|t| t.error.is_some()).count();
    session::save_manifest(&saved.local, &manifest);
    config::save(saved).map_err(|e| Failure::Save(e.to_string()))?;
    Ok((saved.local.clone(), done.len() - failed, failed))
}
