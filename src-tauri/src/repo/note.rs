use crate::models::Note;
use crate::repo::now;
use rusqlite::{params, Connection, Result};

pub fn create(conn: &Connection, title: &str) -> Result<Note> {
    create_with_folder(conn, title, None)
}

/// 新建笔记并可一次性归档到文件夹（单事务路径由调用方持锁）。
pub fn create_with_folder(conn: &Connection, title: &str, folder_id: Option<i64>) -> Result<Note> {
    let ts = now();
    let sort = next_note_sort(conn, folder_id)?;
    conn.execute(
        "INSERT INTO notes (title, folder_id, sort_order, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?4)",
        params![title, folder_id, sort, ts],
    )?;
    get(conn, conn.last_insert_rowid())
}

pub fn set_source_path(conn: &Connection, note_id: i64, source_path: Option<&str>) -> Result<Note> {
    let affected = conn.execute(
        "UPDATE notes SET source_path = ?1, updated_at = ?2 WHERE id = ?3",
        params![source_path, now(), note_id],
    )?;
    if affected == 0 {
        return Err(rusqlite::Error::InvalidParameterName(format!(
            "NOT_FOUND: 笔记 {note_id} 不存在"
        )));
    }
    get(conn, note_id)
}

pub fn get(conn: &Connection, id: i64) -> Result<Note> {
    conn.query_row(
        "SELECT id, title, content, folder_id, source_path, sort_order, created_at, updated_at FROM notes WHERE id = ?1",
        params![id],
        row_to_note,
    )
}

pub fn list(conn: &Connection) -> Result<Vec<Note>> {
    let mut stmt = conn.prepare(
        "SELECT id, title, content, folder_id, source_path, sort_order, created_at, updated_at FROM notes ORDER BY updated_at DESC, id DESC",
    )?;
    let rows = stmt.query_map([], row_to_note)?;
    rows.collect()
}

/// 笔记列表（仅元信息，不拉 content）：用于外部保存速记后主窗口刷新列表，
/// 避免每次刷新都全量读取正文，数据量大时省内存省 IO。
pub fn list_meta(conn: &Connection) -> Result<Vec<Note>> {
    let mut stmt = conn.prepare(
        "SELECT id, title, '', folder_id, source_path, sort_order, created_at, updated_at FROM notes ORDER BY updated_at DESC, id DESC",
    )?;
    let rows = stmt.query_map([], row_to_note)?;
    rows.collect()
}

pub fn update(conn: &Connection, id: i64, title: &str, content: &str) -> Result<Note> {
    let affected = conn.execute(
        "UPDATE notes SET title = ?1, content = ?2, updated_at = ?3 WHERE id = ?4",
        params![title, content, now(), id],
    )?;
    if affected == 0 {
        // 带 NOT_FOUND 前缀：扩展桥调用方据此与服务器错误区分，不再盲目重试
        return Err(rusqlite::Error::InvalidParameterName(format!(
            "NOT_FOUND: 笔记 {id} 不存在"
        )));
    }
    get(conn, id)
}

pub fn set_folder(conn: &Connection, note_id: i64, folder_id: Option<i64>) -> Result<Note> {
    let sort = next_note_sort(conn, folder_id)?;
    let affected = conn.execute(
        "UPDATE notes SET folder_id = ?1, sort_order = ?2, updated_at = ?3 WHERE id = ?4",
        params![folder_id, sort, now(), note_id],
    )?;
    if affected == 0 {
        return Err(rusqlite::Error::InvalidParameterName(format!(
            "NOT_FOUND: 笔记 {note_id} 不存在"
        )));
    }
    get(conn, note_id)
}

pub fn delete(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM notes WHERE id = ?1", params![id])?;
    Ok(())
}

pub fn find_by_source_path(conn: &Connection, path: &str) -> Result<Option<Note>> {
    let mut stmt = conn.prepare(
        "SELECT id, title, content, folder_id, source_path, sort_order, created_at, updated_at FROM notes WHERE source_path = ?1 LIMIT 1",
    )?;
    let mut rows = stmt.query(params![path])?;
    match rows.next()? {
        Some(row) => Ok(Some(row_to_note(row)?)),
        None => Ok(None),
    }
}

pub fn create_imported(
    conn: &Connection,
    title: &str,
    content: &str,
    folder_id: Option<i64>,
    source_path: &str,
) -> Result<Note> {
    let ts = now();
    let sort = next_note_sort(conn, folder_id)?;
    conn.execute(
        "INSERT INTO notes (title, content, folder_id, source_path, sort_order, created_at, updated_at) VALUES (?1,?2,?3,?4,?5,?6,?6)",
        params![title, content, folder_id, source_path, sort, ts],
    )?;
    get(conn, conn.last_insert_rowid())
}

pub fn update_imported(
    conn: &Connection,
    id: i64,
    title: &str,
    content: &str,
    folder_id: Option<i64>,
) -> Result<Note> {
    let affected = conn.execute(
        "UPDATE notes SET title=?1, content=?2, folder_id=?3, updated_at=?4 WHERE id=?5",
        params![title, content, folder_id, now(), id],
    )?;
    if affected == 0 {
        return Err(rusqlite::Error::InvalidParameterName(format!(
            "NOT_FOUND: 笔记 {id} 不存在"
        )));
    }
    get(conn, id)
}

pub fn search(conn: &Connection, keyword: &str) -> Result<Vec<Note>> {
    let pattern = format!("%{}%", keyword);
    let mut stmt = conn.prepare(
        "SELECT id, title, content, folder_id, source_path, sort_order, created_at, updated_at FROM notes WHERE title LIKE ?1 OR content LIKE ?1 ORDER BY updated_at DESC",
    )?;
    let rows = stmt.query_map(params![pattern], row_to_note)?;
    rows.collect()
}

/// 按传入顺序写入手动排序位（同 folder_id 分组内拖拽排序；ids[i] 的 sort_order = i+1）。
pub fn reorder(conn: &Connection, ids: &[i64]) -> Result<()> {
    if ids.is_empty() {
        return Ok(());
    }
    let mut expected_folder: Option<Option<i64>> = None;
    for &id in ids {
        let note = get(conn, id)?;
        match expected_folder {
            None => expected_folder = Some(note.folder_id),
            Some(ref fid) if *fid == note.folder_id => {}
            _ => {
                return Err(rusqlite::Error::InvalidParameterName(
                    "INVALID_ARGUMENT: 笔记须属同一文件夹".into(),
                ));
            }
        }
    }
    let ts = now();
    for (i, &id) in ids.iter().enumerate() {
        conn.execute(
            "UPDATE notes SET sort_order = ?1, updated_at = ?2 WHERE id = ?3",
            params![(i as i64) + 1, ts, id],
        )?;
    }
    Ok(())
}

fn next_note_sort(conn: &Connection, folder_id: Option<i64>) -> Result<i64> {
    let max: Option<i64> = match folder_id {
        Some(fid) => conn.query_row(
            "SELECT MAX(sort_order) FROM notes WHERE folder_id = ?1",
            params![fid],
            |r| r.get(0),
        )?,
        None => conn.query_row(
            "SELECT MAX(sort_order) FROM notes WHERE folder_id IS NULL",
            [],
            |r| r.get(0),
        )?,
    };
    Ok(max.unwrap_or(0) + 1)
}

pub fn row_to_note(row: &rusqlite::Row) -> Result<Note> {
    Ok(Note {
        id: row.get(0)?,
        title: row.get(1)?,
        content: row.get(2)?,
        folder_id: row.get(3)?,
        source_path: row.get(4)?,
        sort_order: row.get(5)?,
        created_at: row.get(6)?,
        updated_at: row.get(7)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_in_memory;

    #[test]
    fn create_and_get_note() {
        let conn = init_in_memory().unwrap();
        let n = create(&conn, "待办事项").unwrap();
        assert_eq!(n.title, "待办事项");
        assert_eq!(n.content, "");
        assert_eq!(n.folder_id, None);
        assert_eq!(n.source_path, None);
    }

    #[test]
    fn list_notes_ordered_by_updated_desc() {
        let conn = init_in_memory().unwrap();
        let a = create(&conn, "A").unwrap();
        let b = create(&conn, "B").unwrap();
        update(&conn, a.id, "A", "updated later").unwrap();
        let list = list(&conn).unwrap();
        assert_eq!(list.iter().map(|n| n.id).collect::<Vec<_>>(), vec![a.id, b.id]);
    }

    #[test]
    fn update_note_title_and_content() {
        let conn = init_in_memory().unwrap();
        let n = create(&conn, "T").unwrap();
        let updated = update(&conn, n.id, "新标题", "这是内容").unwrap();
        assert_eq!(updated.title, "新标题");
        assert_eq!(updated.content, "这是内容");
    }

    #[test]
    fn delete_note() {
        let conn = init_in_memory().unwrap();
        let n = create(&conn, "T").unwrap();
        delete(&conn, n.id).unwrap();
        assert!(get(&conn, n.id).is_err());
    }

    #[test]
    fn search_notes_by_title_and_content() {
        let conn = init_in_memory().unwrap();
        create(&conn, "购物清单").unwrap();
        let second = create(&conn, "会议记录").unwrap();
        update(&conn, second.id, "会议记录", "讨论了发布计划").unwrap();
        let by_title = search(&conn, "购物").unwrap();
        assert_eq!(by_title.len(), 1);
        let by_content = search(&conn, "发布计划").unwrap();
        assert_eq!(by_content.len(), 1);
        assert_eq!(by_content[0].id, second.id);
    }

    #[test]
    fn create_with_folder_sets_folder_id() {
        let conn = init_in_memory().unwrap();
        let f = crate::repo::folder::create(&conn, None, "工", None).unwrap();
        let n = create_with_folder(&conn, "t", Some(f.id)).unwrap();
        assert_eq!(n.folder_id, Some(f.id));
    }

    #[test]
    fn source_path_unique_rejects_duplicate() {
        let conn = init_in_memory().unwrap();
        create_imported(&conn, "a", "1", None, "same.md").unwrap();
        let err = create_imported(&conn, "b", "2", None, "same.md").unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("UNIQUE") || msg.contains("unique"),
            "expected unique constraint, got: {msg}"
        );
    }

    #[test]
    fn set_source_path_roundtrip() {
        let conn = init_in_memory().unwrap();
        let n = create(&conn, "t").unwrap();
        let n2 = set_source_path(&conn, n.id, Some("docs/a.md")).unwrap();
        assert_eq!(n2.source_path.as_deref(), Some("docs/a.md"));
        let n3 = set_source_path(&conn, n.id, None).unwrap();
        assert_eq!(n3.source_path, None);
    }

    #[test]
    fn create_assigns_incrementing_sort_in_folder() {
        let conn = init_in_memory().unwrap();
        let f = crate::repo::folder::create(&conn, None, "W", None).unwrap();
        let a = create_with_folder(&conn, "a", Some(f.id)).unwrap();
        let b = create_with_folder(&conn, "b", Some(f.id)).unwrap();
        assert_eq!(a.sort_order, 1);
        assert_eq!(b.sort_order, 2);
    }

    #[test]
    fn reorder_notes_same_folder_rewrites_sort() {
        let conn = init_in_memory().unwrap();
        let f = crate::repo::folder::create(&conn, None, "W", None).unwrap();
        let a = create_with_folder(&conn, "a", Some(f.id)).unwrap();
        let b = create_with_folder(&conn, "b", Some(f.id)).unwrap();
        reorder(&conn, &[b.id, a.id]).unwrap();
        assert_eq!(get(&conn, b.id).unwrap().sort_order, 1);
        assert_eq!(get(&conn, a.id).unwrap().sort_order, 2);
    }

    #[test]
    fn reorder_notes_rejects_mixed_folders() {
        let conn = init_in_memory().unwrap();
        let f1 = crate::repo::folder::create(&conn, None, "A", None).unwrap();
        let f2 = crate::repo::folder::create(&conn, None, "B", None).unwrap();
        let a = create_with_folder(&conn, "a", Some(f1.id)).unwrap();
        let b = create_with_folder(&conn, "b", Some(f2.id)).unwrap();
        let err = reorder(&conn, &[a.id, b.id]).unwrap_err();
        assert!(err.to_string().contains("INVALID") || err.to_string().contains("不同"));
    }
}
