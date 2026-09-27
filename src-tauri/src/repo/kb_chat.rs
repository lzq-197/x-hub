use crate::models::{KbMessage, KbProject, KbSession};
use crate::repo::now;
use rusqlite::{params, Connection, Result};

const SESSION_COLS: &str =
    "id, title, model_name, project_id, pinned, created_at, updated_at";

const MESSAGE_COLS: &str = "id, session_id, role, content, citations_json, created_at";

const PROJECT_COLS: &str = "id, name, sort_order, created_at, updated_at";

// --- Projects ---

pub fn list_projects(conn: &Connection) -> Result<Vec<KbProject>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {PROJECT_COLS} FROM kb_projects ORDER BY sort_order ASC, id ASC"
    ))?;
    let rows = stmt.query_map([], row_to_project)?;
    rows.collect()
}

pub fn create_project(conn: &Connection, name: &str) -> Result<KbProject> {
    let ts = now();
    conn.execute(
        "INSERT INTO kb_projects (name, sort_order, created_at, updated_at)
         VALUES (?1, COALESCE((SELECT MAX(sort_order) FROM kb_projects), 0) + 1, ?2, ?2)",
        params![name, ts],
    )?;
    get_project(conn, conn.last_insert_rowid())
}

pub fn rename_project(conn: &Connection, id: i64, name: &str) -> Result<KbProject> {
    conn.execute(
        "UPDATE kb_projects SET name = ?1, updated_at = ?2 WHERE id = ?3",
        params![name, now(), id],
    )?;
    get_project(conn, id)
}

pub fn delete_project(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM kb_projects WHERE id = ?1", params![id])?;
    Ok(())
}

fn get_project(conn: &Connection, id: i64) -> Result<KbProject> {
    conn.query_row(
        &format!("SELECT {PROJECT_COLS} FROM kb_projects WHERE id = ?1"),
        params![id],
        row_to_project,
    )
}

// --- Sessions ---

pub fn list_sessions(conn: &Connection) -> Result<Vec<KbSession>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {SESSION_COLS} FROM kb_sessions ORDER BY updated_at DESC"
    ))?;
    let rows = stmt.query_map([], row_to_session)?;
    rows.collect()
}

pub fn get_session(conn: &Connection, id: i64) -> Result<KbSession> {
    conn.query_row(
        &format!("SELECT {SESSION_COLS} FROM kb_sessions WHERE id = ?1"),
        params![id],
        row_to_session,
    )
}

pub fn create_session(
    conn: &Connection,
    title: &str,
    model_name: &str,
    project_id: Option<i64>,
) -> Result<KbSession> {
    let ts = now();
    conn.execute(
        "INSERT INTO kb_sessions (title, model_name, project_id, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?4)",
        params![title, model_name, project_id, ts],
    )?;
    get_session(conn, conn.last_insert_rowid())
}

pub fn rename_session(conn: &Connection, id: i64, title: &str) -> Result<KbSession> {
    conn.execute(
        "UPDATE kb_sessions SET title = ?1, updated_at = ?2 WHERE id = ?3",
        params![title, now(), id],
    )?;
    get_session(conn, id)
}

pub fn set_session_model(conn: &Connection, id: i64, model_name: &str) -> Result<KbSession> {
    conn.execute(
        "UPDATE kb_sessions SET model_name = ?1, updated_at = ?2 WHERE id = ?3",
        params![model_name, now(), id],
    )?;
    get_session(conn, id)
}

pub fn set_pinned(conn: &Connection, id: i64, pinned: bool) -> Result<KbSession> {
    conn.execute(
        "UPDATE kb_sessions SET pinned = ?1, updated_at = ?2 WHERE id = ?3",
        params![pinned as i64, now(), id],
    )?;
    get_session(conn, id)
}

pub fn move_to_project(conn: &Connection, id: i64, project_id: Option<i64>) -> Result<KbSession> {
    conn.execute(
        "UPDATE kb_sessions SET project_id = ?1, updated_at = ?2 WHERE id = ?3",
        params![project_id, now(), id],
    )?;
    get_session(conn, id)
}

pub fn delete_session(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM kb_sessions WHERE id = ?1", params![id])?;
    Ok(())
}

pub fn clear_sessions(conn: &Connection) -> Result<()> {
    conn.execute("DELETE FROM kb_sessions", [])?;
    Ok(())
}

pub fn touch_session(conn: &Connection, id: i64) -> Result<()> {
    conn.execute(
        "UPDATE kb_sessions SET updated_at = ?1 WHERE id = ?2",
        params![now(), id],
    )?;
    Ok(())
}

// --- Messages ---

pub fn list_messages(conn: &Connection, session_id: i64) -> Result<Vec<KbMessage>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {MESSAGE_COLS} FROM kb_messages
         WHERE session_id = ?1 ORDER BY id ASC"
    ))?;
    let rows = stmt.query_map(params![session_id], row_to_message)?;
    rows.collect()
}

pub fn add_message(
    conn: &Connection,
    session_id: i64,
    role: &str,
    content: &str,
    citations_json: Option<&str>,
) -> Result<KbMessage> {
    let ts = now();
    conn.execute(
        "INSERT INTO kb_messages (session_id, role, content, citations_json, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![session_id, role, content, citations_json, ts],
    )?;
    get_message(conn, conn.last_insert_rowid())
}

pub fn get_message(conn: &Connection, id: i64) -> Result<KbMessage> {
    conn.query_row(
        &format!("SELECT {MESSAGE_COLS} FROM kb_messages WHERE id = ?1"),
        params![id],
        row_to_message,
    )
}

// --- Helpers ---

pub fn auto_title_from_question(q: &str) -> String {
    let collapsed: String = q
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let title: String = collapsed.chars().take(30).collect();
    if title.is_empty() {
        "新对话".to_string()
    } else {
        title
    }
}

fn row_to_project(row: &rusqlite::Row) -> Result<KbProject> {
    Ok(KbProject {
        id: row.get(0)?,
        name: row.get(1)?,
        sort_order: row.get(2)?,
        created_at: row.get(3)?,
        updated_at: row.get(4)?,
    })
}

fn row_to_session(row: &rusqlite::Row) -> Result<KbSession> {
    Ok(KbSession {
        id: row.get(0)?,
        title: row.get(1)?,
        model_name: row.get(2)?,
        project_id: row.get(3)?,
        pinned: row.get::<_, i64>(4)? != 0,
        created_at: row.get(5)?,
        updated_at: row.get(6)?,
    })
}

fn row_to_message(row: &rusqlite::Row) -> Result<KbMessage> {
    Ok(KbMessage {
        id: row.get(0)?,
        session_id: row.get(1)?,
        role: row.get(2)?,
        content: row.get(3)?,
        citations_json: row.get(4)?,
        created_at: row.get(5)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_in_memory;

    #[test]
    fn project_delete_nulls_session_project_id() {
        let conn = init_in_memory().unwrap();
        let p = create_project(&conn, "开源").unwrap();
        let s = create_session(&conn, "新对话", "m", Some(p.id)).unwrap();
        assert_eq!(s.project_id, Some(p.id));
        delete_project(&conn, p.id).unwrap();
        let s2 = get_session(&conn, s.id).unwrap();
        assert_eq!(s2.project_id, None);
        assert!(list_projects(&conn).unwrap().is_empty());
    }

    #[test]
    fn message_cascade_on_session_delete() {
        let conn = init_in_memory().unwrap();
        let s = create_session(&conn, "测试", "m", None).unwrap();
        add_message(&conn, s.id, "user", "hi", None).unwrap();
        add_message(&conn, s.id, "assistant", "hello", Some("[]")).unwrap();
        assert_eq!(list_messages(&conn, s.id).unwrap().len(), 2);
        delete_session(&conn, s.id).unwrap();
        assert!(list_messages(&conn, s.id).unwrap().is_empty());
    }

    #[test]
    fn clear_sessions_keeps_projects() {
        let conn = init_in_memory().unwrap();
        let p = create_project(&conn, "项目").unwrap();
        create_session(&conn, "s1", "m", Some(p.id)).unwrap();
        create_session(&conn, "s2", "m", None).unwrap();
        clear_sessions(&conn).unwrap();
        assert!(list_sessions(&conn).unwrap().is_empty());
        assert_eq!(list_projects(&conn).unwrap().len(), 1);
    }

    #[test]
    fn auto_title_from_question_truncates() {
        let long = "a".repeat(40);
        let title = auto_title_from_question(&long);
        assert_eq!(title.len(), 30);
    }

    #[test]
    fn set_pinned_and_move() {
        let conn = init_in_memory().unwrap();
        let p = create_project(&conn, "p1").unwrap();
        let s = create_session(&conn, "t", "m", None).unwrap();
        assert!(!s.pinned);
        let pinned = set_pinned(&conn, s.id, true).unwrap();
        assert!(pinned.pinned);
        let moved = move_to_project(&conn, s.id, Some(p.id)).unwrap();
        assert_eq!(moved.project_id, Some(p.id));
    }
}
