import { computed, type Ref } from 'vue'
import type { NoteFolder } from '../api/tauri'
import type { AppSelectOption } from '../components/AppSelect.vue'

export type FolderFilter = 'all' | 'uncategorized' | number

/** 按 parent_id 分组并排序的文件夹子映射 */
export function useFolderChildren(folders: Ref<readonly NoteFolder[]> | { value: readonly NoteFolder[] }) {
  const childrenOf = computed(() => {
    const map = new Map<number | null, NoteFolder[]>()
    for (const f of folders.value) {
      const key = f.parent_id
      const list = map.get(key) ?? []
      list.push(f)
      map.set(key, list)
    }
    for (const list of map.values()) {
      list.sort((a, b) => a.sort_order - b.sort_order || a.name.localeCompare(b.name, 'zh'))
    }
    return map
  })

  function flatFolderOptions(
    excludeId: number | null = null,
    blocked?: Set<number>,
  ): AppSelectOption[] {
    const opts: AppSelectOption[] = []
    const walk = (parentId: number | null, depth: number) => {
      for (const c of childrenOf.value.get(parentId) ?? []) {
        if (blocked?.has(c.id)) continue
        if (excludeId != null && c.id === excludeId) {
          walk(c.id, depth)
          continue
        }
        opts.push({
          value: String(c.id),
          label: `${'　'.repeat(depth)}${c.name}`,
        })
        walk(c.id, depth + 1)
      }
    }
    walk(null, 0)
    return opts
  }

  function descendantIds(id: number): Set<number> {
    const out = new Set<number>()
    const walk = (pid: number) => {
      for (const c of childrenOf.value.get(pid) ?? []) {
        out.add(c.id)
        walk(c.id)
      }
    }
    walk(id)
    return out
  }

  /** 从某文件夹向上收集祖先 id（含自身） */
  function ancestorIds(folderId: number): number[] {
    const byId = new Map(folders.value.map((f) => [f.id, f]))
    const out: number[] = []
    let cur: number | null = folderId
    while (cur != null) {
      out.push(cur)
      cur = byId.get(cur)?.parent_id ?? null
    }
    return out
  }

  return { childrenOf, flatFolderOptions, descendantIds, ancestorIds }
}
