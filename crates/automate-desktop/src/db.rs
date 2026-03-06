use anyhow::Result;
use chrono::Utc;
use rusqlite::{params, Connection};
use uuid::Uuid;

use automate_shared::models::VmProfile;

pub fn init_db(path: &str) -> Result<Connection> {
    let conn = Connection::open(path)?;
    run_migrations(&conn)?;
    Ok(conn)
}

fn run_migrations(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS vm_profiles (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            host TEXT NOT NULL,
            port INTEGER NOT NULL,
            user TEXT NOT NULL,
            key_path TEXT NOT NULL,
            arch TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );",
    )?;
    Ok(())
}

pub fn insert_vm(conn: &Connection, profile: &VmProfile) -> Result<()> {
    conn.execute(
        "INSERT INTO vm_profiles (id, name, host, port, user, key_path, arch, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            profile.id.to_string(),
            profile.name,
            profile.host,
            profile.port,
            profile.user,
            profile.key_path,
            profile.arch,
            profile.created_at.to_rfc3339(),
            profile.updated_at.to_rfc3339(),
        ],
    )?;
    Ok(())
}

pub fn update_vm(conn: &Connection, profile: &VmProfile) -> Result<()> {
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE vm_profiles SET name=?1, host=?2, port=?3, user=?4, key_path=?5, arch=?6, updated_at=?7 WHERE id=?8",
        params![
            profile.name,
            profile.host,
            profile.port,
            profile.user,
            profile.key_path,
            profile.arch,
            now,
            profile.id.to_string(),
        ],
    )?;
    Ok(())
}

pub fn delete_vm(conn: &Connection, id: &Uuid) -> Result<()> {
    conn.execute(
        "DELETE FROM vm_profiles WHERE id=?1",
        params![id.to_string()],
    )?;
    Ok(())
}

fn row_to_vm_profile(row: &rusqlite::Row) -> rusqlite::Result<VmProfile> {
    Ok(VmProfile {
        id: row
            .get::<_, String>(0)?
            .parse()
            .unwrap_or_else(|_| Uuid::nil()),
        name: row.get(1)?,
        host: row.get(2)?,
        port: row.get::<_, i64>(3)? as u16,
        user: row.get(4)?,
        key_path: row.get(5)?,
        arch: row.get(6)?,
        created_at: row
            .get::<_, String>(7)?
            .parse()
            .unwrap_or_else(|_| Utc::now()),
        updated_at: row
            .get::<_, String>(8)?
            .parse()
            .unwrap_or_else(|_| Utc::now()),
    })
}

pub fn list_vms(conn: &Connection) -> Result<Vec<VmProfile>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, host, port, user, key_path, arch, created_at, updated_at FROM vm_profiles ORDER BY created_at DESC",
    )?;
    let rows = stmt.query_map([], row_to_vm_profile)?;
    let mut vms = Vec::new();
    for row in rows {
        vms.push(row?);
    }
    Ok(vms)
}

pub fn get_vm(conn: &Connection, id: &Uuid) -> Result<Option<VmProfile>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, host, port, user, key_path, arch, created_at, updated_at FROM vm_profiles WHERE id=?1",
    )?;
    let mut rows = stmt.query_map(params![id.to_string()], row_to_vm_profile)?;
    match rows.next() {
        Some(row) => Ok(Some(row?)),
        None => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();
        conn
    }

    fn make_vm(name: &str, host: &str) -> VmProfile {
        let now = Utc::now();
        VmProfile {
            id: Uuid::new_v4(),
            name: name.to_string(),
            host: host.to_string(),
            port: 22,
            user: "root".to_string(),
            key_path: "/home/user/.ssh/id_rsa".to_string(),
            arch: None,
            created_at: now,
            updated_at: now,
        }
    }

    #[test]
    fn test_insert_and_get() {
        let conn = setup_db();
        let vm = make_vm("test-vm", "192.168.1.100");
        insert_vm(&conn, &vm).unwrap();

        let fetched = get_vm(&conn, &vm.id).unwrap().unwrap();
        assert_eq!(fetched.name, "test-vm");
        assert_eq!(fetched.host, "192.168.1.100");
        assert_eq!(fetched.port, 22);
    }

    #[test]
    fn test_list_vms() {
        let conn = setup_db();
        insert_vm(&conn, &make_vm("vm-1", "10.0.0.1")).unwrap();
        insert_vm(&conn, &make_vm("vm-2", "10.0.0.2")).unwrap();

        let vms = list_vms(&conn).unwrap();
        assert_eq!(vms.len(), 2);
    }

    #[test]
    fn test_update_vm() {
        let conn = setup_db();
        let mut vm = make_vm("old-name", "10.0.0.1");
        insert_vm(&conn, &vm).unwrap();

        vm.name = "new-name".to_string();
        vm.host = "10.0.0.2".to_string();
        update_vm(&conn, &vm).unwrap();

        let fetched = get_vm(&conn, &vm.id).unwrap().unwrap();
        assert_eq!(fetched.name, "new-name");
        assert_eq!(fetched.host, "10.0.0.2");
    }

    #[test]
    fn test_delete_vm() {
        let conn = setup_db();
        let vm = make_vm("to-delete", "10.0.0.1");
        insert_vm(&conn, &vm).unwrap();

        delete_vm(&conn, &vm.id).unwrap();
        let fetched = get_vm(&conn, &vm.id).unwrap();
        assert!(fetched.is_none());
    }

    #[test]
    fn test_get_nonexistent() {
        let conn = setup_db();
        let result = get_vm(&conn, &Uuid::new_v4()).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_insert_with_arch() {
        let conn = setup_db();
        let mut vm = make_vm("arch-vm", "10.0.0.1");
        vm.arch = Some("aarch64".to_string());
        insert_vm(&conn, &vm).unwrap();

        let fetched = get_vm(&conn, &vm.id).unwrap().unwrap();
        assert_eq!(fetched.arch, Some("aarch64".to_string()));
    }

    #[test]
    fn test_duplicate_insert_fails() {
        let conn = setup_db();
        let vm = make_vm("dup", "10.0.0.1");
        insert_vm(&conn, &vm).unwrap();
        assert!(insert_vm(&conn, &vm).is_err());
    }
}
