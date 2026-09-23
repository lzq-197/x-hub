//! 笔记变更后的索引钩子（本段为 stub；RAG 段替换实现，不改调用点）。

/// 笔记新建 / 更新 / 导入成功后调用。当前无副作用。
pub fn on_notes_changed(note_ids: &[i64]) {
    // RAG 段替换：真正的 kb_index_note / 队列
    for &id in note_ids {
        let _ = crate::knowledge::index_note_stub(id);
    }
}
