//! 笔记变更后的索引钩子：spawn 增量索引，失败仅 log。

use std::sync::OnceLock;
use tauri::AppHandle;

static APP: OnceLock<AppHandle> = OnceLock::new();

pub fn init(app: AppHandle) {
    let _ = APP.set(app);
}

/// 笔记新建 / 更新 / 导入成功后调用。
pub fn on_notes_changed(note_ids: &[i64]) {
    let Some(app) = APP.get() else {
        return;
    };
    for &id in note_ids {
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            if let Err(e) = crate::knowledge::index_note_async(&app, id).await {
                log::warn!("kb index note {id}: {e}");
            }
        });
    }
}
