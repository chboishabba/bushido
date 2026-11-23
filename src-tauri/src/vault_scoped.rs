//! Linux scoped credential records. Legacy rows never qualify for native fill.
use super::*;
use chacha20poly1305::aead::Payload;

const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS vault_scoped_v2 (
    id TEXT PRIMARY KEY NOT NULL,
    workspace TEXT NOT NULL,
    origin TEXT NOT NULL,
    username TEXT NOT NULL,
    password_enc BLOB NOT NULL,
    nonce BLOB NOT NULL CHECK(length(nonce) = 12),
    updated_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_vault_scoped_origin ON vault_scoped_v2(workspace, origin);";

pub(super) fn init(conn: &Connection) -> Result<(), String> {
    conn.execute_batch(SCHEMA).map_err(|_| "Scoped vault schema initialization failed".into())
}

pub(super) fn workspace(raw: Option<&str>) -> Result<&str, String> {
    raw.filter(|s| !s.is_empty() && s.len() <= 256 && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'))
        .ok_or_else(|| "A valid explicit workspace is required".into())
}

// Domain-separated, unambiguous length-prefixed bytes. Bind every field used
// for credential routing/selection. No ambient default scope or legacy retry.
fn aad(id: &str, workspace: &str, origin: &str, username: &str) -> Vec<u8> {
    let mut out = b"bushido/vault/scoped/v2\0".to_vec();
    for s in [id, workspace, origin, username] {
        out.extend_from_slice(&(s.len() as u64).to_be_bytes());
        out.extend_from_slice(s.as_bytes());
    }
    out
}

fn seal(key: &[u8; 32], nonce: &[u8; 12], metadata: &[u8], password: &str) -> Result<Vec<u8>, String> {
    ChaCha20Poly1305::new(GenericArray::from_slice(key))
        .encrypt(GenericArray::from_slice(nonce), Payload { msg: password.as_bytes(), aad: metadata })
        .map_err(|_| "Scoped vault encryption failed".into())
}

fn open(key: &[u8; 32], nonce: &[u8], metadata: &[u8], cipher: &[u8]) -> Result<Zeroizing<String>, String> {
    if nonce.len() != 12 { return Err("Invalid scoped vault nonce".into()); }
    let plaintext = ChaCha20Poly1305::new(GenericArray::from_slice(key))
        .decrypt(GenericArray::from_slice(nonce), Payload { msg: cipher, aad: metadata })
        .map_err(|_| "Scoped vault authentication failed".to_owned())?;
    String::from_utf8(plaintext).map(Zeroizing::new).map_err(|e| {
        let mut bytes = e.into_bytes(); bytes.zeroize(); "Invalid scoped vault plaintext".into()
    })
}

pub(super) fn save(state: &VaultState, workspace: &str, origin: &str, user: &str, pass: &str) -> Result<String, String> {
    let epoch = state.session()?;
    save_admitted(state, workspace, origin, user, pass, epoch, false, || true)
}

// Consent is bound to the original epoch; never reacquire a new authorization
// after a lock/reunlock while encryption or platform wrapping is in flight.
pub(super) fn save_admitted(state: &VaultState, workspace: &str, origin: &str, user: &str, pass: &str,
    epoch: u64, only_new: bool, permitted: impl Fn() -> bool) -> Result<String, String> {
    state.with_session(epoch, || ())?;
    if !permitted() { return Err("Login save authorization expired".into()); }
    let key = state.get_key()?;
    let id = Uuid::new_v4().to_string();
    let mut nonce = [0u8; 12];
    rand::thread_rng().fill(&mut nonce);
    let cipher = seal(&key, &nonce, &aad(&id, workspace, origin, user), pass)?;
    let wrapped = dpapi_encrypt(&cipher)?;
    let conn = state.open_db()?;
    state.with_session(epoch, || -> Result<(), String> {
        if !permitted() { return Err("Login save authorization expired".into()); }
        let count = conn.execute(
            "INSERT INTO vault_scoped_v2 (id,workspace,origin,username,password_enc,nonce,updated_at)
             SELECT ?1,?2,?3,?4,?5,?6,?7 WHERE NOT ?8 OR NOT EXISTS
             (SELECT 1 FROM vault_scoped_v2 WHERE workspace=?2 AND origin=?3 AND username=?4)",
            rusqlite::params![id, workspace, origin, user, wrapped, nonce.as_slice(), chrono::Utc::now().timestamp(), only_new]
        ).map_err(|_| "Scoped vault insert failed".to_owned())?;
        if count != 1 { return Err("Saved account already exists; no password overwritten".into()); }
        Ok(())
    })??;
    Ok(id)
}

pub(super) fn list(conn: &Connection, workspace: &str, origin: Option<&str>) -> Result<Vec<VaultEntryMetadata>, String> {
    let mut stmt = conn.prepare("SELECT id, origin, username FROM vault_scoped_v2 WHERE workspace = ?1 AND (?2 IS NULL OR origin = ?2) ORDER BY origin, updated_at DESC")
        .map_err(|_| "Scoped metadata query failed".to_owned())?;
    let rows = stmt.query_map(rusqlite::params![workspace, origin], |row| Ok(VaultEntryMetadata {
        id: row.get(0)?, domain: row.get(1)?, username: row.get(2)?,
    })).map_err(|_| "Scoped metadata query failed".to_owned())?;
    rows.collect::<Result<Vec<_>, _>>().map_err(|_| "Scoped metadata read failed".into())
}

pub(super) fn selected(state: &VaultState, workspace: &str, origin: Option<&str>, id: &str, expected_user: Option<&str>, epoch: u64)
    -> Result<(String, Zeroizing<String>), String> {
    state.with_session(epoch, || ())?;
    let conn = state.open_db()?;
    let (actual_origin, user, wrapped, nonce): (String, String, Vec<u8>, Vec<u8>) = conn.query_row(
        "SELECT origin, username, password_enc, nonce FROM vault_scoped_v2 WHERE workspace = ?1 AND id = ?2 AND (?3 IS NULL OR origin = ?3)",
        rusqlite::params![workspace, id, origin], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
    ).map_err(|_| "Selected scoped account not found".to_owned())?;
    if expected_user.is_some_and(|expected| expected != user.as_str()) {
        return Err("Account changed; select again".into());
    }
    let key = state.get_key()?;
    let cipher = dpapi_decrypt(&wrapped)?;
    let pass = open(&key, &nonce, &aad(id, workspace, &actual_origin, &user), &cipher)?;
    state.with_session(epoch, || (user, pass))
}

pub(super) fn delete(state: &VaultState, workspace: &str, id: &str, epoch: u64) -> Result<(), String> {
    let conn = state.open_db()?;
    state.with_session(epoch, || {
        let count = conn.execute("DELETE FROM vault_scoped_v2 WHERE workspace = ?1 AND id = ?2",
            rusqlite::params![workspace, id]).map_err(|_| "Scoped vault delete failed".to_owned())?;
        if count != 1 { return Err("Selected scoped account not found".into()); }
        Ok(())
    })?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aad_binds_identity_workspace_origin_and_username() {
        let key = [7; 32]; let nonce = [8; 12];
        let fields = ["id", "ws-a", "https://example.test:8443", "alice"];
        let ciphertext = seal(&key, &nonce, &aad(fields[0], fields[1], fields[2], fields[3]), "synthetic").unwrap();
        assert_eq!(open(&key, &nonce, &aad(fields[0], fields[1], fields[2], fields[3]), &ciphertext).unwrap().as_str(), "synthetic");
        for i in 0..4 {
            let mut changed = fields; changed[i] = "different";
            assert!(open(&key, &nonce, &aad(changed[0], changed[1], changed[2], changed[3]), &ciphertext).is_err());
        }
        assert_ne!(aad("ab","c","d","e"), aad("a","bc","d","e"));
        assert!(open(&key, &[0;11], &aad(fields[0], fields[1], fields[2], fields[3]), &ciphertext).is_err());
    }

    #[test]
    fn schema_keeps_scopes_and_legacy_separate() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE vault_entries (id TEXT); INSERT INTO vault_entries VALUES ('legacy');").unwrap();
        init(&conn).unwrap(); init(&conn).unwrap();
        for (id, ws, origin) in [("one","ws-a","https://a.test"),("two","ws-b","https://a.test"),("three","ws-a","https://a.test:8443")] {
            conn.execute("INSERT INTO vault_scoped_v2 VALUES (?1,?2,?3,'u',X'00',zeroblob(12),0)", rusqlite::params![id, ws, origin]).unwrap();
        }
        assert_eq!(list(&conn, "ws-a", None).unwrap().len(), 2);
        assert_eq!(list(&conn, "ws-a", Some("https://a.test")).unwrap()[0].id, "one");
        assert_eq!(list(&conn, "ws-b", Some("https://a.test")).unwrap()[0].id, "two");
        assert!(list(&conn, "ws-c", None).unwrap().is_empty());
        assert_eq!(conn.query_row("SELECT count(*) FROM vault_entries", [], |r| r.get::<_, i32>(0)).unwrap(), 1);
    }
}
