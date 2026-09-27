use crate::models::{
    KbDragSourceZone, KbMessage, KbPlaceTarget, KbProject, KbSession, KbSessionZone,
};
use crate::repo::now;
use rusqlite::{params, Connection, Result};

const SESSION_COLS: &str =
    "id, title, model_name, project_id, pinned, sort_order, pin_sort_order, created_at, updated_at";

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
    // 删前记下将被打回「最近」的孤儿（非置顶），稳定序；删后接到现有最近末尾再重密（§3.3）
    let mut orphan_stmt = conn.prepare(
        "SELECT id FROM kb_sessions
         WHERE project_id = ?1 AND pinned = 0
         ORDER BY sort_order ASC, id ASC",
    )?;
    let orphan_ids: Vec<i64> = orphan_stmt
        .query_map(params![id], |r| r.get(0))?
        .collect::<Result<Vec<_>>>()?;
    let recent_max: i64 = conn.query_row(
        "SELECT COALESCE(MAX(sort_order), 0) FROM kb_sessions
         WHERE project_id IS NULL AND pinned = 0",
        [],
        |r| r.get(0),
    )?;

    conn.execute("DELETE FROM kb_projects WHERE id = ?1", params![id])?;
    // ON DELETE SET NULL 后：孤儿接到最近末尾，再 densify，避免按旧 sort 与既有最近交错
    for (i, sid) in orphan_ids.iter().enumerate() {
        conn.execute(
            "UPDATE kb_sessions SET sort_order = ?1 WHERE id = ?2",
            params![recent_max + (i as i64) + 1, sid],
        )?;
    }
    densify_recent_visible(conn)?;
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
    let sort_order = next_membership_sort(conn, project_id)?;
    conn.execute(
        "INSERT INTO kb_sessions (title, model_name, project_id, sort_order, pin_sort_order, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, 0, ?5, ?5)",
        params![title, model_name, project_id, sort_order, ts],
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
    let session = get_session(conn, id)?;
    let ts = now();
    if pinned {
        let pin_sort = if session.pinned {
            session.pin_sort_order
        } else {
            next_pin_sort(conn)?
        };
        conn.execute(
            "UPDATE kb_sessions SET pinned = 1, pin_sort_order = ?1, updated_at = ?2 WHERE id = ?3",
            params![pin_sort, ts, id],
        )?;
    } else {
        conn.execute(
            "UPDATE kb_sessions SET pinned = 0, pin_sort_order = 0, updated_at = ?1 WHERE id = ?2",
            params![ts, id],
        )?;
        densify_pinned(conn)?;
    }
    get_session(conn, id)
}

pub fn move_to_project(conn: &Connection, id: i64, project_id: Option<i64>) -> Result<KbSession> {
    let session = get_session(conn, id)?;
    if session.project_id == project_id {
        return Ok(session);
    }
    let old_project_id = session.project_id;
    let sort_order = next_membership_sort(conn, project_id)?;
    conn.execute(
        "UPDATE kb_sessions SET project_id = ?1, sort_order = ?2, updated_at = ?3 WHERE id = ?4",
        params![project_id, sort_order, now(), id],
    )?;
    densify_membership(conn, old_project_id)?;
    densify_membership(conn, project_id)?;
    get_session(conn, id)
}

/// 区内按传入顺序写回密序。`Pinned` 写 `pin_sort_order`，其余写 `sort_order`。
pub fn reorder_sessions(
    conn: &Connection,
    zone: KbSessionZone,
    project_id: Option<i64>,
    ids: &[i64],
) -> Result<()> {
    if ids.is_empty() {
        return Ok(());
    }
    match zone {
        KbSessionZone::Pinned => {
            for &id in ids {
                if !get_session(conn, id)?.pinned {
                    return Err(rusqlite::Error::InvalidParameterName(
                        "INVALID_ARGUMENT: 重排序列含非置顶会话".into(),
                    ));
                }
            }
            for (i, &id) in ids.iter().enumerate() {
                conn.execute(
                    "UPDATE kb_sessions SET pin_sort_order = ?1 WHERE id = ?2",
                    params![(i as i64) + 1, id],
                )?;
            }
        }
        KbSessionZone::Recent => {
            for &id in ids {
                let s = get_session(conn, id)?;
                if s.pinned || s.project_id.is_some() {
                    return Err(rusqlite::Error::InvalidParameterName(
                        "INVALID_ARGUMENT: 重排序列含非最近区会话".into(),
                    ));
                }
            }
            for (i, &id) in ids.iter().enumerate() {
                conn.execute(
                    "UPDATE kb_sessions SET sort_order = ?1 WHERE id = ?2",
                    params![(i as i64) + 1, id],
                )?;
            }
        }
        KbSessionZone::Project => {
            let pid = project_id.ok_or_else(|| {
                rusqlite::Error::InvalidParameterName(
                    "INVALID_ARGUMENT: project zone requires project_id".into(),
                )
            })?;
            for &id in ids {
                if get_session(conn, id)?.project_id != Some(pid) {
                    return Err(rusqlite::Error::InvalidParameterName(
                        "INVALID_ARGUMENT: 重排序列含非该项目会话".into(),
                    ));
                }
            }
            for (i, &id) in ids.iter().enumerate() {
                conn.execute(
                    "UPDATE kb_sessions SET sort_order = ?1 WHERE id = ?2",
                    params![(i as i64) + 1, id],
                )?;
            }
        }
    }
    Ok(())
}

/// 聚合落点：事务内改 pinned / project_id，并按目标区缝重密序。
pub fn place_session(
    conn: &Connection,
    id: i64,
    source_zone: KbDragSourceZone,
    target: &KbPlaceTarget,
) -> Result<KbSession> {
    let tx = conn.unchecked_transaction()?;
    let session = get_session(&tx, id)?;
    let old_project_id = session.project_id;
    let old_pinned = session.pinned;

    let (new_pinned, new_project_id) = match target.zone {
        KbSessionZone::Pinned => (true, session.project_id),
        KbSessionZone::Recent => (false, None),
        KbSessionZone::Project => {
            let pid = target.project_id.ok_or_else(|| {
                rusqlite::Error::InvalidParameterName(
                    "INVALID_ARGUMENT: project zone requires project_id".into(),
                )
            })?;
            let pinned = if source_zone == KbDragSourceZone::Pinned {
                false
            } else {
                session.pinned
            };
            (pinned, Some(pid))
        }
    };

    let mut ids = zone_member_ids_excluding(&tx, target.zone, new_project_id, id)?;
    insert_before(&mut ids, id, target.before_id);

    tx.execute(
        "UPDATE kb_sessions SET pinned = ?1, project_id = ?2 WHERE id = ?3",
        params![new_pinned as i64, new_project_id, id],
    )?;

    if target.zone == KbSessionZone::Pinned {
        for (i, &sid) in ids.iter().enumerate() {
            tx.execute(
                "UPDATE kb_sessions SET pin_sort_order = ?1 WHERE id = ?2",
                params![(i as i64) + 1, sid],
            )?;
        }
    } else {
        for (i, &sid) in ids.iter().enumerate() {
            tx.execute(
                "UPDATE kb_sessions SET sort_order = ?1 WHERE id = ?2",
                params![(i as i64) + 1, sid],
            )?;
        }
        if old_project_id != new_project_id {
            densify_membership(&tx, old_project_id)?;
        }
        if old_pinned && !new_pinned {
            tx.execute(
                "UPDATE kb_sessions SET pin_sort_order = 0 WHERE id = ?1",
                params![id],
            )?;
            densify_pinned(&tx)?;
        }
    }

    tx.execute(
        "UPDATE kb_sessions SET updated_at = ?1 WHERE id = ?2",
        params![now(), id],
    )?;
    tx.commit()?;
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

// --- Sort helpers ---

fn next_membership_sort(conn: &Connection, project_id: Option<i64>) -> Result<i64> {
    let max: Option<i64> = match project_id {
        Some(pid) => conn.query_row(
            "SELECT MAX(sort_order) FROM kb_sessions WHERE project_id = ?1",
            params![pid],
            |r| r.get(0),
        )?,
        None => conn.query_row(
            "SELECT MAX(sort_order) FROM kb_sessions WHERE project_id IS NULL",
            [],
            |r| r.get(0),
        )?,
    };
    Ok(max.unwrap_or(0) + 1)
}

fn next_pin_sort(conn: &Connection) -> Result<i64> {
    let max: Option<i64> = conn.query_row(
        "SELECT MAX(pin_sort_order) FROM kb_sessions WHERE pinned = 1",
        [],
        |r| r.get(0),
    )?;
    Ok(max.unwrap_or(0) + 1)
}

/// 重写归属桶 `sort_order` 为 1..n（`project_id` 相同；NULL 桶 = `project_id IS NULL`）。
fn densify_membership(conn: &Connection, project_id: Option<i64>) -> Result<()> {
    let sql = match project_id {
        Some(_) => {
            "SELECT id FROM kb_sessions WHERE project_id = ?1 ORDER BY sort_order ASC, id ASC"
        }
        None => {
            "SELECT id FROM kb_sessions WHERE project_id IS NULL ORDER BY sort_order ASC, id ASC"
        }
    };
    let mut stmt = conn.prepare(sql)?;
    let ids: Vec<i64> = match project_id {
        Some(pid) => stmt
            .query_map(params![pid], |r| r.get(0))?
            .collect::<Result<Vec<_>>>()?,
        None => stmt
            .query_map([], |r| r.get(0))?
            .collect::<Result<Vec<_>>>()?,
    };
    for (i, id) in ids.iter().enumerate() {
        conn.execute(
            "UPDATE kb_sessions SET sort_order = ?1 WHERE id = ?2",
            params![(i as i64) + 1, id],
        )?;
    }
    Ok(())
}

/// 重写置顶区 `pin_sort_order` 为 1..n。
fn densify_pinned(conn: &Connection) -> Result<()> {
    let mut stmt = conn.prepare(
        "SELECT id FROM kb_sessions WHERE pinned = 1 ORDER BY pin_sort_order ASC, id ASC",
    )?;
    let ids: Vec<i64> = stmt
        .query_map([], |r| r.get(0))?
        .collect::<Result<Vec<_>>>()?;
    for (i, id) in ids.iter().enumerate() {
        conn.execute(
            "UPDATE kb_sessions SET pin_sort_order = ?1 WHERE id = ?2",
            params![(i as i64) + 1, id],
        )?;
    }
    Ok(())
}

/// 删项目后：仅重密最近可见行。
fn densify_recent_visible(conn: &Connection) -> Result<()> {
    let mut stmt = conn.prepare(
        "SELECT id FROM kb_sessions
         WHERE project_id IS NULL AND pinned = 0
         ORDER BY sort_order ASC, id ASC",
    )?;
    let ids: Vec<i64> = stmt
        .query_map([], |r| r.get(0))?
        .collect::<Result<Vec<_>>>()?;
    for (i, id) in ids.iter().enumerate() {
        conn.execute(
            "UPDATE kb_sessions SET sort_order = ?1 WHERE id = ?2",
            params![(i as i64) + 1, id],
        )?;
    }
    Ok(())
}

fn zone_member_ids_excluding(
    conn: &Connection,
    zone: KbSessionZone,
    project_id: Option<i64>,
    exclude_id: i64,
) -> Result<Vec<i64>> {
    match zone {
        KbSessionZone::Pinned => {
            let mut stmt = conn.prepare(
                "SELECT id FROM kb_sessions
                 WHERE pinned = 1 AND id != ?1
                 ORDER BY pin_sort_order ASC, id ASC",
            )?;
            let ids = stmt
                .query_map(params![exclude_id], |r| r.get(0))?
                .collect::<Result<Vec<_>>>()?;
            Ok(ids)
        }
        KbSessionZone::Recent => {
            let mut stmt = conn.prepare(
                "SELECT id FROM kb_sessions
                 WHERE pinned = 0 AND project_id IS NULL AND id != ?1
                 ORDER BY sort_order ASC, id ASC",
            )?;
            let ids = stmt
                .query_map(params![exclude_id], |r| r.get(0))?
                .collect::<Result<Vec<_>>>()?;
            Ok(ids)
        }
        KbSessionZone::Project => {
            let pid = project_id.ok_or_else(|| {
                rusqlite::Error::InvalidParameterName(
                    "INVALID_ARGUMENT: project zone requires project_id".into(),
                )
            })?;
            let mut stmt = conn.prepare(
                "SELECT id FROM kb_sessions
                 WHERE project_id = ?1 AND id != ?2
                 ORDER BY sort_order ASC, id ASC",
            )?;
            let ids = stmt
                .query_map(params![pid, exclude_id], |r| r.get(0))?
                .collect::<Result<Vec<_>>>()?;
            Ok(ids)
        }
    }
}

fn insert_before(ids: &mut Vec<i64>, id: i64, before_id: Option<i64>) {
    match before_id {
        Some(before) => {
            if let Some(pos) = ids.iter().position(|&x| x == before) {
                ids.insert(pos, id);
            } else {
                ids.push(id);
            }
        }
        None => ids.push(id),
    }
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
        sort_order: row.get(5)?,
        pin_sort_order: row.get(6)?,
        created_at: row.get(7)?,
        updated_at: row.get(8)?,
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

    #[test]
    fn create_appends_sort_in_bucket() {
        let conn = init_in_memory().unwrap();
        let a = create_session(&conn, "a", "m", None).unwrap();
        let b = create_session(&conn, "b", "m", None).unwrap();
        assert_eq!(a.sort_order, 1);
        assert_eq!(b.sort_order, 2);
    }

    #[test]
    fn place_into_project_from_recent() {
        let conn = init_in_memory().unwrap();
        let p = create_project(&conn, "P").unwrap();
        let s = create_session(&conn, "t", "m", None).unwrap();
        let out = place_session(
            &conn,
            s.id,
            KbDragSourceZone::Recent,
            &KbPlaceTarget {
                zone: KbSessionZone::Project,
                project_id: Some(p.id),
                before_id: None,
            },
        )
        .unwrap();
        assert_eq!(out.project_id, Some(p.id));
        assert!(!out.pinned);
        assert_eq!(out.sort_order, 1);
    }

    #[test]
    fn place_from_pinned_zone_to_project_unpins() {
        let conn = init_in_memory().unwrap();
        let p = create_project(&conn, "P").unwrap();
        let s = create_session(&conn, "t", "m", Some(p.id)).unwrap();
        set_pinned(&conn, s.id, true).unwrap();
        let out = place_session(
            &conn,
            s.id,
            KbDragSourceZone::Pinned,
            &KbPlaceTarget {
                zone: KbSessionZone::Project,
                project_id: Some(p.id),
                before_id: None,
            },
        )
        .unwrap();
        assert!(!out.pinned);
        assert_eq!(out.project_id, Some(p.id));
    }

    #[test]
    fn place_to_pinned_keeps_project_id() {
        let conn = init_in_memory().unwrap();
        let p = create_project(&conn, "P").unwrap();
        let s = create_session(&conn, "t", "m", Some(p.id)).unwrap();
        let out = place_session(
            &conn,
            s.id,
            KbDragSourceZone::Project,
            &KbPlaceTarget {
                zone: KbSessionZone::Pinned,
                project_id: None,
                before_id: None,
            },
        )
        .unwrap();
        assert!(out.pinned);
        assert_eq!(out.project_id, Some(p.id));
        assert_eq!(out.pin_sort_order, 1);
    }

    #[test]
    fn place_to_recent_clears_project_and_pin() {
        let conn = init_in_memory().unwrap();
        let p = create_project(&conn, "P").unwrap();
        let s = create_session(&conn, "t", "m", Some(p.id)).unwrap();
        set_pinned(&conn, s.id, true).unwrap();
        let out = place_session(
            &conn,
            s.id,
            KbDragSourceZone::Pinned,
            &KbPlaceTarget {
                zone: KbSessionZone::Recent,
                project_id: None,
                before_id: None,
            },
        )
        .unwrap();
        assert!(!out.pinned);
        assert_eq!(out.project_id, None);
    }

    #[test]
    fn reorder_pinned_independent_of_project_order() {
        let conn = init_in_memory().unwrap();
        let p = create_project(&conn, "P").unwrap();
        let a = create_session(&conn, "a", "m", Some(p.id)).unwrap();
        let b = create_session(&conn, "b", "m", Some(p.id)).unwrap();
        set_pinned(&conn, a.id, true).unwrap();
        set_pinned(&conn, b.id, true).unwrap();
        reorder_sessions(&conn, KbSessionZone::Pinned, None, &[b.id, a.id]).unwrap();
        reorder_sessions(&conn, KbSessionZone::Project, Some(p.id), &[a.id, b.id]).unwrap();
        assert_eq!(get_session(&conn, b.id).unwrap().pin_sort_order, 1);
        assert_eq!(get_session(&conn, a.id).unwrap().pin_sort_order, 2);
        assert_eq!(get_session(&conn, a.id).unwrap().sort_order, 1);
        assert_eq!(get_session(&conn, b.id).unwrap().sort_order, 2);
    }

    #[test]
    fn reorder_rejects_cross_zone() {
        let conn = init_in_memory().unwrap();
        let p = create_project(&conn, "P").unwrap();
        let a = create_session(&conn, "a", "m", None).unwrap();
        let b = create_session(&conn, "b", "m", Some(p.id)).unwrap();
        let err = reorder_sessions(&conn, KbSessionZone::Recent, None, &[a.id, b.id]).unwrap_err();
        assert!(err.to_string().contains("INVALID"));
    }

    #[test]
    fn delete_project_densifies_recent() {
        let conn = init_in_memory().unwrap();
        let p = create_project(&conn, "P").unwrap();
        let r1 = create_session(&conn, "r1", "m", None).unwrap();
        let r2 = create_session(&conn, "r2", "m", None).unwrap();
        let o1 = create_session(&conn, "o1", "m", Some(p.id)).unwrap();
        let o2 = create_session(&conn, "o2", "m", Some(p.id)).unwrap();
        delete_project(&conn, p.id).unwrap();
        assert_eq!(get_session(&conn, o1.id).unwrap().project_id, None);
        assert_eq!(get_session(&conn, o2.id).unwrap().project_id, None);
        // 孤儿接在既有最近之后，再密成 1..n（不得与 r1/r2 按旧 sort 交错）
        let mut recent: Vec<_> = list_sessions(&conn)
            .unwrap()
            .into_iter()
            .filter(|x| !x.pinned && x.project_id.is_none())
            .collect();
        recent.sort_by_key(|x| x.sort_order);
        assert_eq!(recent.len(), 4);
        assert_eq!(
            recent.iter().map(|x| x.id).collect::<Vec<_>>(),
            vec![r1.id, r2.id, o1.id, o2.id]
        );
        assert_eq!(
            recent.iter().map(|x| x.sort_order).collect::<Vec<_>>(),
            vec![1, 2, 3, 4]
        );
    }

    #[test]
    fn place_from_project_keeps_pin_on_project_move() {
        let conn = init_in_memory().unwrap();
        let p1 = create_project(&conn, "P1").unwrap();
        let p2 = create_project(&conn, "P2").unwrap();
        let s = create_session(&conn, "t", "m", Some(p1.id)).unwrap();
        set_pinned(&conn, s.id, true).unwrap();
        // 同源项目内重排：sourceZone=Project 且仍在该项目 → 保持 pinned
        let same = place_session(
            &conn,
            s.id,
            KbDragSourceZone::Project,
            &KbPlaceTarget {
                zone: KbSessionZone::Project,
                project_id: Some(p1.id),
                before_id: None,
            },
        )
        .unwrap();
        assert!(same.pinned);
        assert_eq!(same.project_id, Some(p1.id));
        // 拖到另一项目：sourceZone=Project → 保持 pinned，只换 project_id
        let other = place_session(
            &conn,
            s.id,
            KbDragSourceZone::Project,
            &KbPlaceTarget {
                zone: KbSessionZone::Project,
                project_id: Some(p2.id),
                before_id: None,
            },
        )
        .unwrap();
        assert!(other.pinned);
        assert_eq!(other.project_id, Some(p2.id));
    }

    #[test]
    fn menu_move_appends_to_bucket() {
        let conn = init_in_memory().unwrap();
        let p = create_project(&conn, "P").unwrap();
        let a = create_session(&conn, "a", "m", Some(p.id)).unwrap();
        let b = create_session(&conn, "b", "m", None).unwrap();
        let moved = move_to_project(&conn, b.id, Some(p.id)).unwrap();
        assert_eq!(moved.sort_order, 2);
        assert_eq!(get_session(&conn, a.id).unwrap().sort_order, 1);
    }
}
