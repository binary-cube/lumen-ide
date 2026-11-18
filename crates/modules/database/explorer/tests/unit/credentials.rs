//! The passwords by policy: the project's vault encrypted at rest (its file never holds the
//! text), the keychain, memory for the session or a single connection; moved with their
//! source between policies and between the project and every project. Only memory stores
//! and a temporary folder: never the user's keychain.

use super::*;

struct Fixture {
    dir: PathBuf,
    keys: Arc<MemoryStore>,
    keychain: Arc<MemoryStore>,
    credentials: Credentials,
}

fn fixture(name: &str) -> Fixture {
    let dir = std::env::temp_dir().join(format!("credentials-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let keys = Arc::new(MemoryStore::default());
    let keychain = Arc::new(MemoryStore::default());
    let vault = SourceVaults { dir: dir.clone(), keys: Arc::clone(&keys) as Arc<dyn SecretStore> };
    let credentials =
        Credentials::new(Arc::new(vault), Arc::clone(&keychain) as Arc<dyn SecretStore>);
    Fixture { dir, keys, keychain, credentials }
}

fn source(policy: SecretPolicy, scope: Scope) -> DataSource {
    let mut source = DataSource::new("shop", "postgres", scope);
    source.secrets = policy;
    source
}

fn files(dir: &Path) -> Vec<String> {
    std::fs::read_dir(dir)
        .map(|d| d.filter_map(|e| std::fs::read_to_string(e.ok()?.path()).ok()).collect())
        .unwrap_or_default()
}

#[test]
fn the_project_vault_is_encrypted_at_rest() {
    let f = fixture("vault");
    let shop = source(SecretPolicy::ProjectVault, Scope::Project);
    f.credentials.set(&shop, &Secret::new("p4ss w0rd")).unwrap();
    let written = files(&f.dir);
    assert_eq!(written.len(), 1, "one vault for the source");
    assert!(!written[0].contains("p4ss"), "never the text");
    assert!(!written[0].contains(&hex_of("p4ss w0rd")), "nor its bytes");
    assert_eq!(f.keys.len(), 1, "its key in the keychain");
    assert!(f.keychain.is_empty(), "the password itself isn't there");
    assert_eq!(f.credentials.get(&shop).as_ref().map(Secret::expose), Some("p4ss w0rd"));
    // Removed: the file and its key go.
    f.credentials.remove(&shop).unwrap();
    assert!(files(&f.dir).is_empty());
    assert!(f.keys.is_empty());
    let _ = std::fs::remove_dir_all(&f.dir);
}

fn hex_of(text: &str) -> String {
    text.bytes().map(|b| format!("{b:02x}")).collect()
}

#[test]
fn the_keychain_and_memory_policies() {
    let f = fixture("policies");
    let chain = source(SecretPolicy::Keychain, Scope::Project);
    f.credentials.set(&chain, &Secret::new("k")).unwrap();
    assert_eq!(f.keychain.get(&chain.secret_id()).as_ref().map(Secret::expose), Some("k"));
    let session = source(SecretPolicy::Session, Scope::Project);
    f.credentials.set(&session, &Secret::new("s")).unwrap();
    assert!(f.credentials.has(&session));
    assert_eq!(f.credentials.get(&session).as_ref().map(Secret::expose), Some("s"));
    assert_eq!(f.credentials.get(&session).as_ref().map(Secret::expose), Some("s"), "kept");
    // Asked each time: given to one connection, then forgotten.
    let never = source(SecretPolicy::Never, Scope::Project);
    f.credentials.set(&never, &Secret::new("n")).unwrap();
    assert_eq!(f.credentials.get(&never).as_ref().map(Secret::expose), Some("n"));
    assert_eq!(f.credentials.get(&never), None);
    assert!(!f.credentials.has(&never));
    // Neither memory policy wrote anything anywhere.
    assert_eq!(f.keychain.len(), 1);
    assert!(files(&f.dir).is_empty() && f.keys.is_empty());
    let _ = std::fs::remove_dir_all(&f.dir);
}

#[test]
fn a_global_sources_project_password_is_in_the_keychain() {
    let f = fixture("global");
    let global = source(SecretPolicy::ProjectVault, Scope::Global);
    assert_eq!(effective(&global), SecretPolicy::Keychain);
    f.credentials.set(&global, &Secret::new("g")).unwrap();
    assert!(files(&f.dir).is_empty(), "no project holds it");
    assert_eq!(f.keychain.len(), 1);
}

#[test]
fn moving_a_source_moves_its_password() {
    let f = fixture("move");
    let local = source(SecretPolicy::ProjectVault, Scope::Project);
    f.credentials.set(&local, &Secret::new("m0ve")).unwrap();
    // To every project: from the vault to the keychain.
    let mut global = local.clone();
    global.scope = Scope::Global;
    assert_eq!(f.credentials.moved(&local, &global), Ok(Moved::To(SecretPolicy::Keychain)));
    assert!(files(&f.dir).is_empty(), "gone from the project's vault");
    assert_eq!(f.credentials.get(&global).as_ref().map(Secret::expose), Some("m0ve"));
    // Back to the project, its policy now the session's.
    let mut back = global.clone();
    back.scope = Scope::Project;
    back.secrets = SecretPolicy::Session;
    assert_eq!(f.credentials.moved(&global, &back), Ok(Moved::To(SecretPolicy::Session)));
    assert!(f.keychain.is_empty(), "gone from the keychain");
    assert_eq!(f.credentials.get(&back).as_ref().map(Secret::expose), Some("m0ve"));
    // To "ask each time": dropped.
    let mut never = back.clone();
    never.secrets = SecretPolicy::Never;
    assert_eq!(f.credentials.moved(&back, &never), Ok(Moved::Dropped));
    assert_eq!(f.credentials.get(&never), None);
    // Unchanged: kept.
    assert_eq!(f.credentials.moved(&never, &never), Ok(Moved::Kept));
}
