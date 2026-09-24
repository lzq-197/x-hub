/**
 * 速记树拖放命中区：文件夹行中部 40% = 纳入该夹；上下各 30% = 同级缝。
 * 笔记行上下半 = 该笔记前后缝。纯函数，便于单测。
 */

export type DropAim =
  | { kind: 'nest'; folderId: number }
  | {
      kind: 'folder-gap'
      parentId: number | null
      /** 缝上方兄弟（插入后紧挨其下）；缝在最前则为 null */
      beforeId: number | null
      /** 缝下方兄弟（插入后紧挨其上）；缝在最后则为 null */
      afterId: number | null
    }
  | {
      kind: 'note-gap'
      folderId: number | null
      beforeId: number | null
      afterId: number | null
    }

export type FolderZone = 'above' | 'nest' | 'below'
export type NoteZone = 'above' | 'below'

/** 文件夹行：上 30% / 中 40% / 下 30% */
export function folderZone(
  clientY: number,
  rowTop: number,
  rowHeight: number,
): FolderZone {
  if (rowHeight <= 0) return 'nest'
  const y = clientY - rowTop
  if (y < rowHeight * 0.3) return 'above'
  if (y > rowHeight * 0.7) return 'below'
  return 'nest'
}

/** 笔记行：上/下半当作前后缝（不作为「移入」） */
export function noteZone(clientY: number, rowTop: number, rowHeight: number): NoteZone {
  if (rowHeight <= 0) return 'below'
  return clientY - rowTop < rowHeight / 2 ? 'above' : 'below'
}

/**
 * 在同级 id 列表中按 before/after 缝算出插入下标（相对「已去掉 draggedId」的数组）。
 * beforeId = 缝上方项；afterId = 缝下方项。
 * 当缝贴着被拖项自身时（before/after 为 draggedId），改用另一侧兄弟定位。
 */
export function insertIndexForGap(
  siblingIds: number[],
  draggedId: number,
  beforeId: number | null,
  afterId: number | null,
): number {
  const without = siblingIds.filter((id) => id !== draggedId)
  if (afterId != null && afterId !== draggedId) {
    const i = without.indexOf(afterId)
    if (i >= 0) return i
  }
  if (beforeId != null && beforeId !== draggedId) {
    const i = without.indexOf(beforeId)
    if (i >= 0) return i + 1
  }
  if (beforeId == null) return 0
  return without.length
}

/** 把 draggedId 按缝拼进完整兄弟列表（供 reorder* 整表写入） */
export function spliceForGap(
  siblingIds: number[],
  draggedId: number,
  beforeId: number | null,
  afterId: number | null,
): number[] {
  const without = siblingIds.filter((id) => id !== draggedId)
  const at = insertIndexForGap(siblingIds, draggedId, beforeId, afterId)
  without.splice(at, 0, draggedId)
  return without
}

/**
 * 拖笔记落在文件夹缝时：纳入缝上方那一夹（笔记序末尾）。
 * 缝在最前（无上方夹）→ null，松手应忽略。
 */
export function noteAimFromFolderGap(beforeId: number | null): DropAim | null {
  if (beforeId == null) return null
  return { kind: 'nest', folderId: beforeId }
}

/** 两份顺序是否相同（同 id 同下标） */
export function sameIdOrder(a: number[], b: number[]): boolean {
  if (a.length !== b.length) return false
  for (let i = 0; i < a.length; i++) {
    if (a[i] !== b[i]) return false
  }
  return true
}
