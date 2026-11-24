//! Durable metadata for native downloads. Never a request/resume manifest.
use crate::downloads::{DlItem, DlState};
use rusqlite::Connection;
use std::{path::{Path, PathBuf}, fs, time::Duration};
use tauri::{AppHandle, Manager};

const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS native_history (
    id TEXT PRIMARY KEY NOT NULL,
    item_json TEXT NOT NULL
);";
const UPSERT: &str = "INSERT INTO native_history(id,item_json) VALUES (?1,?2)
    ON CONFLICT(id) DO UPDATE SET item_json=excluded.item_json";
const DELETE: &str = "DELETE FROM native_history WHERE id=?1";

fn connect(app: &AppHandle) -> Result<Connection, String> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt, OpenOptionsExt};
    let root = app.path().app_data_dir().map_err(|_| "No app data directory")?;
    fs::create_dir_all(&root).map_err(|_| "Cannot create app data directory")?;
    let dir = root.join("native-download-history");
    match fs::DirBuilder::new().mode(0o700).create(&dir) {
        Ok(()) => {},
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {},
        Err(_) => return Err("Cannot create native download history directory".into()),
    }
    let attrs = fs::symlink_metadata(&dir).map_err(|_| "Cannot inspect download history directory")?;
    if !attrs.is_dir() || attrs.permissions().mode() & 0o077 != 0 {
        return Err("Native download history directory is not private".into());
    }
    let path = dir.join("history.sqlite3");
    if !path.exists() {
        match fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&path) {
            Ok(_) => {},
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {},
            Err(_) => return Err("Cannot create native download history".into()),
        }
    }
    let attrs = fs::symlink_metadata(&path).map_err(|_| "Cannot inspect download history")?;
    if !attrs.file_type().is_file() || attrs.permissions().mode() & 0o077 != 0 {
        return Err("Native download history file is not private".into());
    }
    let conn = Connection::open(path).map_err(|_| "Cannot open native download history")?;
    conn.busy_timeout(Duration::from_millis(250)).map_err(|_| "Cannot configure download history")?;
    conn.execute_batch("PRAGMA synchronous=FULL;").map_err(|_| "Cannot configure download durability")?;
    conn.execute_batch(SCHEMA).map_err(|_| "Cannot initialize native download history")?;
    Ok(conn)
}

fn valid_id(id: &str) -> bool {
    uuid::Uuid::parse_str(id).is_ok_and(|u| u.to_string() == id)
}

fn stored(item: &DlItem) -> DlItem {
    let mut item = item.clone();
    // Do not retain signed URLs, cookies, body, headers or browser session data.
    item.url.clear();
    item.native = true;
    item.supports_range = false;
    item.segments = 0;
    item.speed = 0;
    // Native error details can contain signed URLs or user-specific paths.
    if item.error.is_some() {
        item.error = Some(if item.state == DlState::Completed {
            "Saved; see the recorded file location".into()
        } else { "Native download failed or was interrupted".into() });
    }
    item
}

fn write(conn: &Connection, item: &DlItem) -> Result<(), String> {
    if !item.native || !valid_id(&item.id) { return Err("Invalid native download record".into()); }
    let snapshot = stored(item);
    let json = serde_json::to_string(&snapshot).map_err(|_| "Cannot encode download history")?;
    conn.execute(UPSERT, rusqlite::params![snapshot.id, json]).map_err(|_| "Cannot save native download history")?;
    Ok(())
}

pub(crate) fn save(app: &AppHandle, item: &DlItem) -> Result<(), String> {
    write(&connect(app)?, item)
}

pub(crate) fn forget(app: &AppHandle, id: &str) -> Result<(), String> {
    if !valid_id(id) { return Err("Invalid native download ID".into()); }
    connect(app)?.execute(DELETE, [id]).map_err(|_| "Cannot remove native download history")?;
    Ok(())
}

fn regular_file(path: &Path) -> Option<fs::Metadata> {
    if !path.is_absolute() { return None; }
    fs::symlink_metadata(path).ok().filter(|m| m.file_type().is_file())
}

// Reconciliation never creates, republishes, resumes, or removes a file.
// A staged file alone is not proof of completion.
fn reconcile(mut item: DlItem) -> DlItem {
    item = stored(&item);
    let path = PathBuf::from(&item.file_path);
    match item.state {
        DlState::Downloading | DlState::Paused => {
            item.state = DlState::Failed;
            let expected = format!(".bushido-download-{}", item.id);
            let private_stage = path.parent().is_some_and(|p| {
                p.file_name().and_then(|n| n.to_str()) == Some(expected.as_str())
                    && fs::symlink_metadata(p).is_ok_and(|m| m.file_type().is_dir())
            });
            if private_stage {
                if let Some(attrs) = regular_file(&path) {
                    item.received_bytes = attrs.len();
                    item.error = Some("Interrupted at restart; partial file retained in private staging. Start again from the original site.".into());
                    return item;
                }
            }
            item.error = Some("Interrupted at restart; no verified partial file. Start again from the original site.".into());
        }
        DlState::Completed => {
            if !regular_file(&path).is_some_and(|attrs| Some(attrs.len()) == item.total_bytes) {
                item.state = DlState::Failed;
                item.error = Some("Previously completed file is missing, replaced or has a different size".into());
            }
        }
        DlState::Failed => {}
    }
    item
}

pub(crate) fn load(app: &AppHandle) -> Result<Vec<DlItem>, String> {
    let conn = connect(app)?;
    let mut stmt = conn.prepare("SELECT id,item_json FROM native_history").map_err(|_| "Cannot query native download history")?;
    let records = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
        .map_err(|_| "Cannot read native download history")?;
    let mut items = Vec::new();
    for record in records {
        let Ok((id, json)) = record else { continue; };
        let Ok(item) = serde_json::from_str::<DlItem>(&json) else { continue; };
        if item.id != id || !item.native || !valid_id(&id) { continue; }
        items.push(reconcile(item));
    }
    // Leave original receipts intact; re-evaluate on each startup. No writes
    // or filesystem changes based solely on interrupted state.
    Ok(items)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn item(path: String) -> DlItem {
        DlItem { id: uuid::Uuid::new_v4().to_string(), url:"https://example.test/private?token=synthetic".into(),
            file_path:path, file_name:"test.bin".into(), mime_type:String::new(), total_bytes:Some(3),
            received_bytes:1, state:DlState::Downloading, speed:7, error:None, created_at:0,
            supports_range:true, native:true, segments:2, priority:0 }
    }
    #[test]
    fn persist_redacts_and_upserts_without_replay_fields() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA).unwrap();
        let mut i = item(String::new());
        write(&conn, &i).unwrap();
        i.received_bytes = 2; write(&conn, &i).unwrap();
        let json: String = conn.query_row("SELECT item_json FROM native_history", [], |r| r.get(0)).unwrap();
        let restored: DlItem = serde_json::from_str(&json).unwrap();
        assert!(restored.url.is_empty() && restored.native && !restored.supports_range);
        assert_eq!(restored.received_bytes, 2);
        assert!(!json.contains("synthetic"));
        assert_eq!(conn.execute(DELETE, [&i.id]).unwrap(), 1);
    }
    #[test]
    fn restart_never_promotes_partial_or_missing_file() {
        let mut i = item("/nonexistent/native-download".into());
        assert!(reconcile(i.clone()).state == DlState::Failed);
        i.state = DlState::Completed;
        assert!(reconcile(i).state == DlState::Failed);
    }
    #[test]
    fn restart_keeps_verified_completed_and_partial_without_deleting() {
        let root = std::env::temp_dir().join(uuid::Uuid::new_v4().to_string());
        fs::create_dir(&root).unwrap();
        let mut i = item(String::new());
        let dir = root.join(format!(".bushido-download-{}", i.id));
        fs::create_dir(&dir).unwrap();
        let path = dir.join("x"); fs::write(&path, b"abc").unwrap();
        i.file_path = path.to_str().unwrap().to_owned();
        let interrupted = reconcile(i.clone());
        assert!(interrupted.state == DlState::Failed);
        assert_eq!(interrupted.received_bytes, 3);
        i.state = DlState::Completed;
        assert!(reconcile(i).state == DlState::Completed);
        assert_eq!(fs::read(path).unwrap(), b"abc");
        fs::remove_dir_all(root).unwrap();
    }
}
