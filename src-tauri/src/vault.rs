use std::{path::PathBuf, sync::atomic::{AtomicU64, Ordering}};
use tauri::Emitter;
use zeroize::{Zeroize, Zeroizing};
use parking_lot::Mutex;
use rusqlite::Connection;
use serde::Serialize;
use uuid::Uuid;
use rand::Rng;
use argon2::{Argon2, PasswordHasher, PasswordVerifier};
use argon2::password_hash::{SaltString, PasswordHash};
use chacha20poly1305::{ChaCha20Poly1305, KeyInit, aead::Aead};
use chacha20poly1305::aead::generic_array::GenericArray;

use crate::sync::keys::{dpapi_encrypt, dpapi_decrypt};
#[cfg(target_os = "linux")]
#[path = "vault_scoped.rs"]
mod scoped;

pub struct VaultState {
    pub db_path: PathBuf,
    // derived key from master password, None = locked
    pub derived_key: Mutex<Option<[u8; 32]>>,
    // Access only while holding derived_key, including transitions.
    epoch: AtomicU64,
}

#[derive(Serialize, Clone)]
pub struct VaultEntry {
    pub id: String,
    pub domain: String,
    pub username: String,
    pub password: String,
    pub notes: String,
    pub created_at: i64,
    pub updated_at: i64,
}

impl VaultState {
    pub fn new(db_path: PathBuf) -> Self {
        Self { db_path, derived_key: Mutex::new(None), epoch: AtomicU64::new(0) }
    }

    pub fn init_db(&self) -> Result<(), String> {
        let conn = self.open_db()?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS vault_entries (
                id TEXT PRIMARY KEY,
                domain TEXT NOT NULL,
                username TEXT NOT NULL,
                password_enc BLOB NOT NULL,
                nonce BLOB NOT NULL,
                notes TEXT DEFAULT '',
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_vault_domain ON vault_entries(domain);
            CREATE TABLE IF NOT EXISTS vault_meta (
                key TEXT PRIMARY KEY,
                value BLOB NOT NULL
            );"
        ).map_err(|e| format!("db init: {}", e))?;
        #[cfg(target_os = "linux")]
        scoped::init(&conn)?;
        Ok(())
    }

    fn open_db(&self) -> Result<Connection, String> {
        Connection::open(&self.db_path).map_err(|e| format!("db open: {}", e))
    }

    // encrypt password: chacha20 then dpapi wrap
    fn encrypt_password(&self, plaintext: &str) -> Result<(Vec<u8>, Vec<u8>), String> {
        let key = self.get_key()?;
        let mut nonce_bytes = [0u8; 12];
        rand::thread_rng().fill(&mut nonce_bytes);
        let nonce = GenericArray::from_slice(&nonce_bytes);
        let cipher = ChaCha20Poly1305::new(GenericArray::from_slice(&*key));
        let encrypted = cipher.encrypt(nonce, plaintext.as_bytes())
            .map_err(|_| "encrypt failed".to_string())?;
        let wrapped = dpapi_encrypt(&encrypted)?;
        Ok((wrapped, nonce_bytes.to_vec()))
    }

    fn decrypt_password(&self, enc: &[u8], nonce: &[u8]) -> Result<String, String> {
        let key = self.get_key()?;
        let unwrapped = dpapi_decrypt(enc)?;
        if nonce.len() != 12 { return Err("invalid vault nonce".into()); }
        let nonce_arr = GenericArray::from_slice(nonce);
        let cipher = ChaCha20Poly1305::new(GenericArray::from_slice(&*key));
        let decrypted = cipher.decrypt(nonce_arr, unwrapped.as_ref())
            .map_err(|_| "decrypt failed".to_string())?;
        String::from_utf8(decrypted).map_err(|_| "invalid utf8".to_string())
    }

    fn get_key(&self) -> Result<Zeroizing<[u8; 32]>, String> {
        let guard = self.derived_key.lock();
        guard.as_ref().map(|key| Zeroizing::new(*key))
            .ok_or_else(|| "vault locked".to_string())
    }

    pub(crate) fn session(&self) -> Result<u64, String> {
        let guard = self.derived_key.lock();
        if guard.is_none() { return Err("Unlock the vault in Settings first".into()); }
        Ok(self.epoch.load(Ordering::Relaxed))
    }

    // Synchronous admission only: do not await, or call a function that
    // re-acquires derived_key, inside this closure.
    pub(crate) fn with_session<T>(&self, expected: u64, action: impl FnOnce() -> T) -> Result<T, String> {
        let guard = self.derived_key.lock();
        if guard.is_none() || self.epoch.load(Ordering::Relaxed) != expected {
            return Err("Vault session changed; authorize again".into());
        }
        Ok(action())
    }

    // Capture a transition generation even while locked, before costly
    // password verification. A newer lock or unlock must win.
    fn generation(&self) -> u64 {
        let _guard = self.derived_key.lock();
        self.epoch.load(Ordering::Relaxed)
    }

    fn status(&self) -> VaultSessionStatus {
        let guard = self.derived_key.lock();
        VaultSessionStatus { epoch: self.epoch.load(Ordering::Relaxed), unlocked: guard.is_some() }
    }

    fn replace_key(&self, key: Option<[u8; 32]>) -> VaultSessionStatus {
        let mut guard = self.derived_key.lock();
        if let Some(old) = guard.as_mut() { old.zeroize(); }
        *guard = key;
        let epoch = self.epoch.fetch_add(1, Ordering::Relaxed) + 1;
        VaultSessionStatus { epoch, unlocked: guard.is_some() }
    }

    fn install_if_current(&self, expected: u64, key: &Zeroizing<[u8; 32]>,
        before_install: impl FnOnce() -> Result<(), String>) -> Result<VaultSessionStatus, String> {
        let mut guard = self.derived_key.lock();
        if self.epoch.load(Ordering::Relaxed) != expected {
            return Err("Vault state changed; retry unlock".into());
        }
        // Initial setup commits its two metadata rows under this admission
        // guard. Failed commit or stale transition does not install a key.
        before_install()?;
        if let Some(old) = guard.as_mut() { old.zeroize(); }
        *guard = Some(**key);
        let epoch = self.epoch.fetch_add(1, Ordering::Relaxed) + 1;
        Ok(VaultSessionStatus { epoch, unlocked: true })
    }
}

#[derive(Serialize, Clone)]
pub struct VaultSessionStatus {
    epoch: u64,
    unlocked: bool,
}

fn publish_status(app: &tauri::AppHandle, status: VaultSessionStatus) {
    let _ = app.emit_to("main", "vault-state-changed", status);
}

#[tauri::command]
pub async fn vault_state_snapshot(state: tauri::State<'_, VaultState>) -> Result<VaultSessionStatus, String> {
    Ok(state.status())
}

// argon2 derive key from master password + salt
fn derive_key(password: &str, salt: &[u8]) -> Result<[u8; 32], String> {
    use argon2::Argon2;
    let mut output = [0u8; 32];
    Argon2::default()
        .hash_password_into(password.as_bytes(), salt, &mut output)
        .map_err(|e| format!("argon2: {}", e))?;
    Ok(output)
}

// sync check if master password is set (no async, no key needed)
pub fn has_master_password_sync(state: &VaultState) -> bool {
    let conn = match state.open_db() {
        Ok(c) => c,
        Err(_) => return false,
    };
    conn.query_row(
        "SELECT COUNT(*) > 0 FROM vault_meta WHERE key = 'master_hash'",
        [],
        |row| row.get::<_, bool>(0),
    ).unwrap_or(false)
}

// Linux native selection is scoped to the actual WebView's creation profile.
#[cfg(target_os = "linux")]
pub(crate) fn native_choices(state: &VaultState, workspace: &str, origin: &str, epoch: u64) -> Result<Vec<(String, String)>, String> {
    state.with_session(epoch, || ())?;
    let conn = state.open_db()?;
    let mut stmt = conn.prepare("SELECT id, username FROM vault_scoped_v2 WHERE workspace = ?1 AND origin = ?2 ORDER BY updated_at DESC LIMIT 51")
        .map_err(|_| "Vault query failed".to_owned())?;
    let choices = stmt.query_map(rusqlite::params![workspace, origin], |row| Ok((row.get(0)?, row.get(1)?)))
        .map_err(|_| "Vault query failed".to_owned())?
        .collect::<Result<Vec<(String, String)>, _>>().map_err(|_| "Vault query failed".to_owned())?;
    if choices.len() > 50 { return Err("Too many matching accounts; use vault Settings".into()); }
    state.with_session(epoch, || choices)
}

#[cfg(target_os = "linux")]
pub(crate) fn native_selected(state: &VaultState, workspace: &str, origin: &str, id: &str, user: &str, epoch: u64) -> Result<(String, Zeroizing<String>), String> {
    scoped::selected(state, workspace, Some(origin), id, Some(user), epoch)
}

#[tauri::command]
pub fn vault_scoped_mode() -> bool { cfg!(target_os = "linux") }

// sync helper for postMessage handler (no async)
pub fn get_entries_for_domain(state: &VaultState, domain: &str) -> Result<Vec<VaultEntry>, String> {
    if state.derived_key.lock().is_none() {
        return Ok(vec![]);
    }
    let conn = state.open_db()?;
    let mut stmt = conn.prepare(
        "SELECT id, domain, username, password_enc, nonce, notes, created_at, updated_at FROM vault_entries WHERE domain = ?1 ORDER BY updated_at DESC"
    ).map_err(|e| format!("query: {}", e))?;
    let rows = stmt.query_map([domain], |row| {
        Ok((
            row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?,
            row.get::<_, Vec<u8>>(3)?, row.get::<_, Vec<u8>>(4)?, row.get::<_, String>(5)?,
            row.get::<_, i64>(6)?, row.get::<_, i64>(7)?,
        ))
    }).map_err(|e| format!("query: {}", e))?;
    let mut entries = Vec::new();
    for row in rows {
        let (id, domain, username, enc, nonce, notes, created_at, updated_at) = row.map_err(|e| format!("row: {}", e))?;
        let password = state.decrypt_password(&enc, &nonce).unwrap_or_default();
        entries.push(VaultEntry { id, domain, username, password, notes, created_at, updated_at });
    }
    Ok(entries)
}

#[tauri::command]
pub async fn vault_has_master_password(state: tauri::State<'_, VaultState>) -> Result<bool, String> {
    let conn = state.open_db()?;
    let exists: bool = conn.query_row(
        "SELECT COUNT(*) > 0 FROM vault_meta WHERE key = 'master_hash'",
        [],
        |row| row.get(0),
    ).unwrap_or(false);
    Ok(exists)
}

#[tauri::command]
pub async fn vault_setup(app: tauri::AppHandle, state: tauri::State<'_, VaultState>, master_password: String) -> Result<(), String> {
    let expected = state.generation();
    let master_password = Zeroizing::new(master_password);
    if master_password.len() < 8 {
        return Err("password must be at least 8 characters".into());
    }

    let salt = SaltString::generate(&mut rand::thread_rng());
    let argon2 = Argon2::default();
    let hash = argon2.hash_password(master_password.as_bytes(), &salt)
        .map_err(|e| format!("hash: {}", e))?;

    let key = Zeroizing::new(derive_key(&master_password, salt.as_str().as_bytes())?);
    let mut conn = state.open_db()?;
    let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(|_| "Vault setup transaction failed".to_owned())?;
    let exists: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM vault_meta WHERE key IN ('master_hash','salt'))", [], |row| row.get(0))
        .map_err(|_| "Vault metadata query failed".to_owned())?;
    if exists { return Err("Vault already initialized; unlock instead".into()); }
    tx.execute(
        "INSERT INTO vault_meta (key, value) VALUES ('master_hash', ?1)",
        [hash.to_string().as_bytes()],
    ).map_err(|e| format!("save hash: {}", e))?;

    // store salt separately for key derivation
    tx.execute(
        "INSERT INTO vault_meta (key, value) VALUES ('salt', ?1)",
        [salt.as_str().as_bytes()],
    ).map_err(|e| format!("save salt: {}", e))?;

    let status = state.install_if_current(expected, &key, ||
        tx.commit().map_err(|_| "Vault setup commit failed".to_owned()))?;
    publish_status(&app, status);

    Ok(())
}

#[tauri::command]
pub async fn vault_unlock(app: tauri::AppHandle, state: tauri::State<'_, VaultState>, master_password: String) -> Result<(), String> {
    let expected = state.generation();
    let master_password = Zeroizing::new(master_password);
    let conn = state.open_db()?;

    let hash_str: String = conn.query_row(
        "SELECT value FROM vault_meta WHERE key = 'master_hash'",
        [],
        |row| {
            let bytes: Vec<u8> = row.get(0)?;
            Ok(String::from_utf8_lossy(&bytes).to_string())
        },
    ).map_err(|_| "no master password set".to_string())?;

    let salt_str: String = conn.query_row(
        "SELECT value FROM vault_meta WHERE key = 'salt'",
        [],
        |row| {
            let bytes: Vec<u8> = row.get(0)?;
            Ok(String::from_utf8_lossy(&bytes).to_string())
        },
    ).map_err(|_| "no salt found".to_string())?;

    // verify password
    let parsed_hash = PasswordHash::new(&hash_str)
        .map_err(|e| format!("parse hash: {}", e))?;
    Argon2::default()
        .verify_password(master_password.as_bytes(), &parsed_hash)
        .map_err(|_| "wrong password".to_string())?;

    let key = Zeroizing::new(derive_key(&master_password, salt_str.as_bytes())?);
    let status = state.install_if_current(expected, &key, || Ok(()))?;
    publish_status(&app, status);

    Ok(())
}

#[tauri::command]
pub async fn vault_lock(app: tauri::AppHandle, state: tauri::State<'_, VaultState>) -> Result<(), String> {
    let status = state.replace_key(None);
    publish_status(&app, status);
    Ok(())
}

#[tauri::command]
pub async fn vault_is_unlocked(state: tauri::State<'_, VaultState>) -> Result<bool, String> {
    let guard = state.derived_key.lock();
    Ok(guard.is_some())
}

#[tauri::command]
pub async fn vault_save_entry(
    state: tauri::State<'_, VaultState>,
    domain: String,
    username: String,
    password: String,
    notes: Option<String>,
) -> Result<String, String> {
    let password = zeroize::Zeroizing::new(password);
    #[cfg(target_os = "linux")]
    {
        let _ = (&state, &domain, &username, &password, &notes);
        return Err("Use workspace-scoped manual login provisioning on Linux".into());
    }
    #[cfg(not(target_os = "linux"))]
    {
    let (enc, nonce) = state.encrypt_password(password.as_str())?;
    let id = Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp();
    let notes = notes.unwrap_or_default();

    let conn = state.open_db()?;
    conn.execute(
        "INSERT INTO vault_entries (id, domain, username, password_enc, nonce, notes, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        rusqlite::params![id, domain, username, enc, nonce, notes, now, now],
    ).map_err(|e| format!("save: {}", e))?;

    Ok(id)
    }
}

// Explicit browser-chrome provisioning, independent of page save prompts.
#[tauri::command]
pub async fn vault_save_manual_entry(
    state: tauri::State<'_, VaultState>,
    site: String,
    username: String,
    password: String,
    workspace_id: Option<String>,
) -> Result<String, String> {
    let password = Zeroizing::new(password);
    if site.len() > 4096 || username.len() > 4096 || password.len() > 65536
        || username.is_empty() || password.is_empty() {
        return Err("Enter a valid HTTPS site, username and password".into());
    }
    // Linux fill never silently upgrades hostname-only legacy credentials:
    // the user explicitly supplies the HTTPS site and its port.
    #[cfg(target_os = "linux")]
    {
        let domain = crate::linux_vault::https_origin(&site)
            .ok_or("Enter an HTTPS URL without embedded credentials")?;
        let workspace = scoped::workspace(workspace_id.as_deref())?;
        return scoped::save(&state, workspace, &domain, &username, password.as_str());
    }
    #[cfg(not(target_os = "linux"))]
    let domain = {
        let url = url::Url::parse(&site).map_err(|_| "Enter a valid HTTPS URL")?;
        if url.scheme() != "https" || !url.username().is_empty() || url.password().is_some() {
            return Err("Enter an HTTPS URL without embedded credentials".into());
        }
        url.host_str().ok_or("Missing hostname")?.to_owned()
    };
    #[cfg(not(target_os = "linux"))]
    { let _ = workspace_id; vault_save_entry(state, domain, username, password.to_string(), None).await }
}

#[derive(Serialize)]
pub struct VaultEntryMetadata {
    id: String,
    domain: String,
    username: String,
}

fn list_metadata(conn: &Connection, domain: Option<&str>) -> Result<Vec<VaultEntryMetadata>, String> {
    let mut stmt = conn.prepare(
        "SELECT id, domain, username FROM vault_entries WHERE (?1 IS NULL OR domain = ?1) ORDER BY domain, updated_at DESC"
    ).map_err(|_| "Vault metadata query failed".to_owned())?;
    let rows = stmt.query_map([domain], |row| Ok(VaultEntryMetadata {
        id: row.get(0)?, domain: row.get(1)?, username: row.get(2)?,
    })).map_err(|_| "Vault metadata query failed".to_owned())?;
    rows.collect::<Result<Vec<_>, _>>().map_err(|_| "Vault metadata read failed".to_owned())
}

#[tauri::command]
pub async fn vault_get_entries(state: tauri::State<'_, VaultState>, domain: Option<String>, workspace_id: Option<String>, legacy: Option<bool>) -> Result<Vec<VaultEntryMetadata>, String> {
    let epoch = state.session()?;
    let conn = state.open_db()?;
    #[cfg(target_os = "linux")]
    if !legacy.unwrap_or(false) {
        let metadata = scoped::list(&conn, scoped::workspace(workspace_id.as_deref())?, domain.as_deref())?;
        return state.with_session(epoch, || metadata);
    }
    let metadata = list_metadata(&conn, domain.as_deref())?;
    state.with_session(epoch, || metadata)
}

fn selected_password(state: &VaultState, id: &str, epoch: u64) -> Result<Zeroizing<String>, String> {
    state.with_session(epoch, || ())?;
    let conn = state.open_db()?;
    let (enc, nonce): (Vec<u8>, Vec<u8>) = conn.query_row(
        "SELECT password_enc, nonce FROM vault_entries WHERE id = ?1",
        [id], |row| Ok((row.get(0)?, row.get(1)?))
    ).map_err(|_| "Selected account not found".to_owned())?;
    let password = Zeroizing::new(state.decrypt_password(&enc, &nonce)?);
    state.with_session(epoch, || password)
}

// Called by trusted Settings only; one explicit reveal, no bulk password list.
#[tauri::command]
pub async fn vault_reveal_entry(state: tauri::State<'_, VaultState>, id: String, workspace_id: Option<String>, legacy: Option<bool>) -> Result<String, String> {
    let epoch = state.session()?;
    #[cfg(target_os = "linux")]
    let password = if legacy.unwrap_or(false) { selected_password(&state, &id, epoch)? }
        else { scoped::selected(&state, scoped::workspace(workspace_id.as_deref())?, None, &id, None, epoch)?.1 };
    #[cfg(not(target_os = "linux"))]
    let password = selected_password(&state, &id, epoch)?;
    state.with_session(epoch, || password.to_string())
}

#[tauri::command]
pub async fn vault_copy_entry(state: tauri::State<'_, VaultState>, id: String, workspace_id: Option<String>, legacy: Option<bool>) -> Result<(), String> {
    let epoch = state.session()?;
    #[cfg(target_os = "linux")]
    let password = if legacy.unwrap_or(false) { selected_password(&state, &id, epoch)? }
        else { scoped::selected(&state, scoped::workspace(workspace_id.as_deref())?, None, &id, None, epoch)?.1 };
    #[cfg(not(target_os = "linux"))]
    let password = selected_password(&state, &id, epoch)?;
    state.with_session(epoch, || {
        let mut clipboard = arboard::Clipboard::new().map_err(|_| "Clipboard unavailable".to_owned())?;
        clipboard.set_text(password.to_string()).map_err(|_| "Clipboard write failed".to_owned())
    })?
}

#[tauri::command]
pub async fn vault_delete_entry(state: tauri::State<'_, VaultState>, id: String, workspace_id: Option<String>, legacy: Option<bool>) -> Result<(), String> {
    let epoch = state.session()?;
    #[cfg(target_os = "linux")]
    if !legacy.unwrap_or(false) {
        return scoped::delete(&state, scoped::workspace(workspace_id.as_deref())?, &id, epoch);
    }
    let conn = state.open_db()?;
    state.with_session(epoch, || conn.execute("DELETE FROM vault_entries WHERE id = ?1", [&id])
        .map_err(|e| format!("delete: {}", e)))??;
    Ok(())
}

#[tauri::command]
pub async fn vault_update_entry(
    state: tauri::State<'_, VaultState>,
    id: String,
    username: Option<String>,
    password: Option<String>,
    notes: Option<String>,
) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        let _ = (&state, &id, &username, &notes);
        let _password = password.map(Zeroizing::new);
        return Err("Legacy credentials cannot be edited on Linux; add a new scoped login".into());
    }
    #[cfg(not(target_os = "linux"))]
    {
    let conn = state.open_db()?;
    let now = chrono::Utc::now().timestamp();

    if let Some(ref pw) = password {
        let (enc, nonce) = state.encrypt_password(pw)?;
        conn.execute(
            "UPDATE vault_entries SET password_enc = ?1, nonce = ?2, updated_at = ?3 WHERE id = ?4",
            rusqlite::params![enc, nonce, now, id],
        ).map_err(|e| format!("update pw: {}", e))?;
    }
    if let Some(ref u) = username {
        conn.execute(
            "UPDATE vault_entries SET username = ?1, updated_at = ?2 WHERE id = ?3",
            rusqlite::params![u, now, id],
        ).map_err(|e| format!("update user: {}", e))?;
    }
    if let Some(ref n) = notes {
        conn.execute(
            "UPDATE vault_entries SET notes = ?1, updated_at = ?2 WHERE id = ?3",
            rusqlite::params![n, now, id],
        ).map_err(|e| format!("update notes: {}", e))?;
    }

    Ok(())
    }
}

#[tauri::command]
pub async fn vault_generate_password(
    length: Option<u32>,
    uppercase: Option<bool>,
    lowercase: Option<bool>,
    digits: Option<bool>,
    symbols: Option<bool>,
) -> Result<String, String> {
    let len = length.unwrap_or(20).max(4).min(128) as usize;
    let uc = uppercase.unwrap_or(true);
    let lc = lowercase.unwrap_or(true);
    let dg = digits.unwrap_or(true);
    let sy = symbols.unwrap_or(true);

    let mut charset = String::new();
    if uc { charset.push_str("ABCDEFGHIJKLMNOPQRSTUVWXYZ"); }
    if lc { charset.push_str("abcdefghijklmnopqrstuvwxyz"); }
    if dg { charset.push_str("0123456789"); }
    if sy { charset.push_str("!@#$%^&*()-_=+[]{}|;:,.<>?"); }
    if charset.is_empty() { charset.push_str("abcdefghijklmnopqrstuvwxyz0123456789"); }

    let chars: Vec<char> = charset.chars().collect();
    let mut rng = rand::thread_rng();
    let password: String = (0..len).map(|_| chars[rng.gen_range(0..chars.len())]).collect();

    Ok(password)
}

#[cfg(test)]
mod session_tests {
    use super::*;

    #[test]
    fn lock_and_reunlock_revoke_old_session() {
        let state = VaultState::new(PathBuf::from("unused"));
        assert!(state.session().is_err());
        state.replace_key(Some([1; 32]));
        let old = state.session().unwrap();
        assert_eq!(state.with_session(old, || 7), Ok(7));
        state.replace_key(None);
        assert!(state.with_session(old, || panic!("revoked")).is_err());
        state.replace_key(Some([1; 32]));
        assert_ne!(old, state.session().unwrap());
        assert!(state.with_session(old, || panic!("revoked")).is_err());
    }

    #[test]
    fn newer_lock_wins_against_pending_unlock_or_setup() {
        let state = VaultState::new(PathBuf::from("unused"));
        let key = Zeroizing::new([1; 32]);
        let pending = state.generation();
        state.replace_key(None);
        assert!(state.install_if_current(pending, &key, || panic!("stale commit")).is_err());
        assert!(state.session().is_err());
        let fresh = state.generation();
        assert!(state.install_if_current(fresh, &key, || Err("commit failed".into())).is_err());
        assert!(state.session().is_err());
        assert!(state.install_if_current(fresh, &key, || Ok(())).is_ok());
        assert!(state.install_if_current(fresh, &key, || panic!("duplicate")).is_err());
    }

    #[test]
    fn metadata_list_does_not_select_or_decrypt_ciphertext() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE vault_entries (id TEXT, domain TEXT, username TEXT, updated_at INTEGER);
            INSERT INTO vault_entries VALUES ('one','https://a.test','user1',1), ('two','https://b.test','user2',2);").unwrap();
        let all = list_metadata(&conn, None).unwrap();
        assert_eq!(all.len(), 2);
        let one = list_metadata(&conn, Some("https://a.test")).unwrap();
        assert_eq!(one.len(), 1);
        let json = serde_json::to_value(&one[0]).unwrap();
        assert_eq!(json["id"], "one");
        assert!(json.get("password").is_none());
    }
}
