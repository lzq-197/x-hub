use crate::models::NoteFolder;
use crate::repo::now;
use rusqlite::{params, Connection, Result};
use std::collections::{HashMap, HashSet};

const MAX_DEPTH: i64 = 8;

pub fn create(
    conn: &Connection,
    parent_id: Option<i64>,
    name: &str,
    sort_order: Option<i64>,
) -> Result<NoteFolder> {
    let name = name.trim();
    if name.is_empty() {
        return Err(rusqlite::Error::InvalidParameterName(
            "INVALID_ARGUMENT: 文件夹名称不能为空".into(),
        ));
    }
    if let Some(pid) = parent_id {
        get(conn, pid).map_err(|_| {
            rusqlite::Error::InvalidParameterName(format!(
                "INVALID_ARGUMENT: 父文件夹 {pid} 不存在"
            ))
        })?;
        if depth_of(conn, pid)? + 1 > MAX_DEPTH {
            return Err(rusqlite::Error::InvalidParameterName(format!(
                "INVALID_ARGUMENT: 文件夹层级不能超过 {MAX_DEPTH} 层"
            )));
        }
    }
    if find_child_by_name(conn, parent_id, name)?.is_some() {
        return Err(rusqlite::Error::InvalidParameterName(
            "INVALID_ARGUMENT: 同级已存在同名文件夹".into(),
        ));
    }

    let sort = sort_order.unwrap_or(next_sibling_sort(conn, parent_id)?);
    let ts = now();
    conn.execute(
        "INSERT INTO note_folders (parent_id, name, sort_order, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?4)",
        params![parent_id, name, sort, ts],
    )?;
    get(conn, conn.last_insert_rowid())
}

pub fn reorder(conn: &Connection, ids: &[i64]) -> Result<()> {
    if ids.is_empty() {
        return Ok(());
    }
    let expected_parent = get(conn, ids[0])?.parent_id;
    for &id in ids {
        let f = get(conn, id)?;
        if f.parent_id != expected_parent {
            return Err(rusqlite::Error::InvalidParameterName(
                "INVALID_ARGUMENT: 只能重排同一父级下的文件夹".into(),
            ));
        }
    }
    for (i, &id) in ids.iter().enumerate() {
        conn.execute(
            "UPDATE note_folders SET sort_order = ?1, updated_at = ?2 WHERE id = ?3",
            params![(i as i64) + 1, now(), id],
        )?;
    }
    Ok(())
}

pub fn rename(conn: &Connection, id: i64, name: &str) -> Result<NoteFolder> {
    let name = name.trim();
    if name.is_empty() {
        return Err(rusqlite::Error::InvalidParameterName(
            "INVALID_ARGUMENT: 文件夹名称不能为空".into(),
        ));
    }
    let current = conn.query_row(
        "SELECT id, parent_id, name, sort_order, created_at, updated_at FROM note_folders WHERE id = ?1",
        params![id],
        row_to_folder_base,
    )?;
    if let Some(existing) = find_child_by_name(conn, current.parent_id, name)? {
        if existing != id {
            return Err(rusqlite::Error::InvalidParameterName(
                "INVALID_ARGUMENT: 同级已存在同名文件夹".into(),
            ));
        }
    }
    let affected = conn.execute(
        "UPDATE note_folders SET name = ?1, updated_at = ?2 WHERE id = ?3",
        params![name, now(), id],
    )?;
    if affected == 0 {
        return Err(rusqlite::Error::InvalidParameterName(format!(
            "NOT_FOUND: 文件夹 {id} 不存在"
        )));
    }
    get(conn, id)
}

pub fn list(conn: &Connection) -> Result<Vec<NoteFolder>> {
    let mut stmt = conn.prepare(
        "SELECT id, parent_id, name, sort_order, created_at, updated_at FROM note_folders ORDER BY sort_order ASC, id ASC",
    )?;
    let rows = stmt.query_map([], row_to_folder_base)?;
    let mut folders: Vec<NoteFolder> = rows.collect::<Result<Vec<_>>>()?;
    attach_counts(conn, &mut folders)?;
    Ok(folders)
}

pub fn get(conn: &Connection, id: i64) -> Result<NoteFolder> {
    let mut folder = conn.query_row(
        "SELECT id, parent_id, name, sort_order, created_at, updated_at FROM note_folders WHERE id = ?1",
        params![id],
        row_to_folder_base,
    )?;
    attach_counts(conn, std::slice::from_mut(&mut folder))?;
    Ok(folder)
}

pub fn move_folder(
    conn: &Connection,
    id: i64,
    new_parent_id: Option<i64>,
) -> Result<NoteFolder> {
    let folder = conn.query_row(
        "SELECT id, parent_id, name, sort_order, created_at, updated_at FROM note_folders WHERE id = ?1",
        params![id],
        row_to_folder_base,
    )?;
    let current_parent = folder.parent_id;

    if new_parent_id == Some(id) {
        return Err(rusqlite::Error::InvalidParameterName(
            "不能将文件夹移动到自身或其子孙文件夹下".into(),
        ));
    }

    if let Some(new_parent) = new_parent_id {
        conn.query_row(
            "SELECT id FROM note_folders WHERE id = ?1",
            params![new_parent],
            |row| row.get::<_, i64>(0),
        )
        .map_err(|_| {
            rusqlite::Error::InvalidParameterName(format!(
                "INVALID_ARGUMENT: 目标父文件夹 {new_parent} 不存在"
            ))
        })?;

        let descendants = collect_descendant_ids(conn, id)?;
        if descendants.contains(&new_parent) {
            return Err(rusqlite::Error::InvalidParameterName(
                "不能将文件夹移动到自身或其子孙文件夹下".into(),
            ));
        }
    }

    let current_depth = depth_of(conn, id)?;
    let max_subtree_depth = max_depth_in_subtree(conn, id)?;
    let subtree_height = max_subtree_depth - current_depth;
    let new_parent_depth = match new_parent_id {
        Some(pid) => depth_of(conn, pid)?,
        None => 0,
    };
    if new_parent_depth + 1 + subtree_height > MAX_DEPTH {
        return Err(rusqlite::Error::InvalidParameterName(format!(
            "INVALID_ARGUMENT: 文件夹层级不能超过 {MAX_DEPTH} 层"
        )));
    }

    let new_sort = if new_parent_id != current_parent {
        next_sibling_sort(conn, new_parent_id)?
    } else {
        folder.sort_order
    };
    let affected = conn.execute(
        "UPDATE note_folders SET parent_id = ?1, sort_order = ?2, updated_at = ?3 WHERE id = ?4",
        params![new_parent_id, new_sort, now(), id],
    )?;
    if affected == 0 {
        return Err(rusqlite::Error::InvalidParameterName(format!(
            "NOT_FOUND: 文件夹 {id} 不存在"
        )));
    }
    get(conn, id)
}

pub fn delete_promote(conn: &Connection, id: i64) -> Result<(i64, i64)> {
    let tx = conn.unchecked_transaction()?;
    let folder = tx.query_row(
        "SELECT id, parent_id, name, sort_order, created_at, updated_at FROM note_folders WHERE id = ?1",
        params![id],
        row_to_folder_base,
    )?;
    let parent = folder.parent_id;
    let promoted: i64 = tx.execute(
        "UPDATE note_folders SET parent_id = ?1, updated_at = ?2 WHERE parent_id = ?3",
        params![parent, now(), id],
    )? as i64;
    let cleared: i64 = tx.execute(
        "UPDATE notes SET folder_id = NULL, updated_at = ?1 WHERE folder_id = ?2",
        params![now(), id],
    )? as i64;
    tx.execute("DELETE FROM note_folders WHERE id = ?1", params![id])?;
    tx.commit()?;
    Ok((cleared, promoted))
}

pub fn collect_descendant_ids(conn: &Connection, id: i64) -> Result<Vec<i64>> {
    let mut ids = vec![id];
    let mut i = 0;
    while i < ids.len() {
        let current = ids[i];
        let mut stmt = conn.prepare("SELECT id FROM note_folders WHERE parent_id = ?1")?;
        let children = stmt.query_map(params![current], |row| row.get(0))?;
        for child in children {
            ids.push(child?);
        }
        i += 1;
    }
    Ok(ids)
}

pub fn find_or_create_path(
    conn: &Connection,
    parent_id: Option<i64>,
    segments: &[&str],
) -> Result<i64> {
    let mut current = parent_id;
    for segment in segments {
        let name = segment.trim();
        if name.is_empty() {
            continue;
        }
        current = Some(find_or_create_one(conn, current, name)?);
    }
    match current {
        Some(id) => Ok(id),
        None => Err(rusqlite::Error::InvalidParameterName(
            "INVALID_ARGUMENT: 路径不能为空".into(),
        )),
    }
}

pub fn depth_of(conn: &Connection, id: i64) -> Result<i64> {
    let mut depth = 0i64;
    let mut current = Some(id);
    let mut seen = HashSet::new();
    while let Some(fid) = current {
        if !seen.insert(fid) {
            return Err(rusqlite::Error::InvalidParameterName(
                "INVALID_ARGUMENT: 文件夹树存在环".into(),
            ));
        }
        depth += 1;
        current = conn
            .query_row(
                "SELECT parent_id FROM note_folders WHERE id = ?1",
                params![fid],
                |row| row.get(0),
            )
            .map_err(|_| {
                rusqlite::Error::InvalidParameterName(format!(
                    "NOT_FOUND: 文件夹 {fid} 不存在"
                ))
            })?;
    }
    Ok(depth)
}

fn next_sibling_sort(conn: &Connection, parent_id: Option<i64>) -> Result<i64> {
    let max: Option<i64> = match parent_id {
        Some(pid) => conn.query_row(
            "SELECT MAX(sort_order) FROM note_folders WHERE parent_id = ?1",
            params![pid],
            |r| r.get(0),
        )?,
        None => conn.query_row(
            "SELECT MAX(sort_order) FROM note_folders WHERE parent_id IS NULL",
            [],
            |r| r.get(0),
        )?,
    };
    Ok(max.unwrap_or(0) + 1)
}

fn find_or_create_one(conn: &Connection, parent_id: Option<i64>, name: &str) -> Result<i64> {
    if let Some(existing) = find_child_by_name(conn, parent_id, name)? {
        return Ok(existing);
    }
    Ok(create(conn, parent_id, name, None)?.id)
}

fn find_child_by_name(
    conn: &Connection,
    parent_id: Option<i64>,
    name: &str,
) -> Result<Option<i64>> {
    let result = match parent_id {
        Some(pid) => conn.query_row(
            "SELECT id FROM note_folders WHERE parent_id = ?1 AND name = ?2",
            params![pid, name],
            |row| row.get(0),
        ),
        None => conn.query_row(
            "SELECT id FROM note_folders WHERE parent_id IS NULL AND name = ?1",
            params![name],
            |row| row.get(0),
        ),
    };
    match result {
        Ok(id) => Ok(Some(id)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e),
    }
}

fn max_depth_in_subtree(conn: &Connection, id: i64) -> Result<i64> {
    let ids = collect_descendant_ids(conn, id)?;
    let mut max = 0i64;
    for fid in ids {
        max = max.max(depth_of(conn, fid)?);
    }
    Ok(max)
}

fn attach_counts(conn: &Connection, folders: &mut [NoteFolder]) -> Result<()> {
    if folders.is_empty() {
        return Ok(());
    }

    let mut direct_counts: HashMap<i64, i64> = HashMap::new();
    let mut stmt = conn.prepare("SELECT folder_id, COUNT(*) FROM notes WHERE folder_id IS NOT NULL GROUP BY folder_id")?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
    })?;
    for row in rows {
        let (fid, count) = row?;
        direct_counts.insert(fid, count);
    }

    let mut subtree_counts: HashMap<i64, i64> = HashMap::new();
    for folder in folders.iter() {
        let descendants = collect_descendant_ids(conn, folder.id)?;
        let total: i64 = descendants
            .iter()
            .map(|fid| direct_counts.get(fid).copied().unwrap_or(0))
            .sum();
        subtree_counts.insert(folder.id, total);
    }

    for folder in folders.iter_mut() {
        folder.notes_count = direct_counts.get(&folder.id).copied().unwrap_or(0);
        folder.subtree_notes = subtree_counts.get(&folder.id).copied().unwrap_or(0);
    }
    Ok(())
}

fn row_to_folder_base(row: &rusqlite::Row) -> Result<NoteFolder> {
    Ok(NoteFolder {
        id: row.get(0)?,
        parent_id: row.get(1)?,
        name: row.get(2)?,
        sort_order: row.get(3)?,
        notes_count: 0,
        subtree_notes: 0,
        created_at: row.get(4)?,
        updated_at: row.get(5)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_in_memory;
    use crate::repo::note;

    #[test]
    fn create_assigns_incrementing_sort_among_siblings() {
        let conn = init_in_memory().unwrap();
        let a = create(&conn, None, "A", None).unwrap();
        let b = create(&conn, None, "B", None).unwrap();
        assert_eq!(a.sort_order, 1);
        assert_eq!(b.sort_order, 2);
    }

    #[test]
    fn reorder_root_folders_rewrites_sort_order() {
        let conn = init_in_memory().unwrap();
        let a = create(&conn, None, "A", None).unwrap();
        let b = create(&conn, None, "B", None).unwrap();
        let c = create(&conn, None, "C", None).unwrap();
        reorder(&conn, &[c.id, a.id, b.id]).unwrap();
        let list = list(&conn).unwrap();
        let by_id: std::collections::HashMap<_, _> =
            list.iter().map(|f| (f.id, f.sort_order)).collect();
        assert_eq!(by_id[&c.id], 1);
        assert_eq!(by_id[&a.id], 2);
        assert_eq!(by_id[&b.id], 3);
    }

    #[test]
    fn reorder_nested_siblings() {
        let conn = init_in_memory().unwrap();
        let root = create(&conn, None, "R", None).unwrap();
        let a = create(&conn, Some(root.id), "A", None).unwrap();
        let b = create(&conn, Some(root.id), "B", None).unwrap();
        reorder(&conn, &[b.id, a.id]).unwrap();
        assert_eq!(get(&conn, b.id).unwrap().sort_order, 1);
        assert_eq!(get(&conn, a.id).unwrap().sort_order, 2);
    }

    #[test]
    fn reorder_rejects_mixed_parents() {
        let conn = init_in_memory().unwrap();
        let r1 = create(&conn, None, "R1", None).unwrap();
        let r2 = create(&conn, None, "R2", None).unwrap();
        let c = create(&conn, Some(r1.id), "C", None).unwrap();
        let err = reorder(&conn, &[r2.id, c.id]).unwrap_err();
        assert!(err.to_string().contains("INVALID") || err.to_string().contains("父"));
    }

    #[test]
    fn move_folder_appends_sort_under_new_parent() {
        let conn = init_in_memory().unwrap();
        let b = create(&conn, None, "B", None).unwrap();
        let _x = create(&conn, Some(b.id), "X", None).unwrap();
        let a = create(&conn, None, "A", None).unwrap();
        move_folder(&conn, a.id, Some(b.id)).unwrap();
        let kids = list(&conn)
            .unwrap()
            .into_iter()
            .filter(|f| f.parent_id == Some(b.id))
            .collect::<Vec<_>>();
        assert_eq!(kids.len(), 2);
        assert_eq!(get(&conn, a.id).unwrap().sort_order, 2);
    }

    #[test]
    fn delete_promotes_children_and_unclassifies_direct_notes() {
        let conn = init_in_memory().unwrap();
        let root = create(&conn, None, "工作", None).unwrap();
        let child = create(&conn, Some(root.id), "项目", None).unwrap();
        let n = note::create(&conn, "直属").unwrap();
        note::set_folder(&conn, n.id, Some(root.id)).unwrap();
        let n2 = note::create(&conn, "子内").unwrap();
        note::set_folder(&conn, n2.id, Some(child.id)).unwrap();

        let (cleared, promoted) = delete_promote(&conn, root.id).unwrap();
        assert_eq!(cleared, 1);
        assert_eq!(promoted, 1);
        assert!(get(&conn, root.id).is_err());
        let child2 = get(&conn, child.id).unwrap();
        assert_eq!(child2.parent_id, None);
        assert_eq!(note::get(&conn, n.id).unwrap().folder_id, None);
        assert_eq!(note::get(&conn, n2.id).unwrap().folder_id, Some(child.id));
    }

    #[test]
    fn move_rejects_cycle() {
        let conn = init_in_memory().unwrap();
        let a = create(&conn, None, "A", None).unwrap();
        let b = create(&conn, Some(a.id), "B", None).unwrap();
        let err = move_folder(&conn, a.id, Some(b.id)).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("环") || msg.contains("子孙") || msg.contains("INVALID"));
    }

    #[test]
    fn create_rejects_sibling_duplicate_name() {
        let conn = init_in_memory().unwrap();
        create(&conn, None, "工作", None).unwrap();
        let err = create(&conn, None, "工作", None).unwrap_err();
        assert!(err.to_string().contains("同名"));
    }
}
