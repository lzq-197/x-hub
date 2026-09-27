import type { Node as ProseNode } from '@milkdown/kit/prose/model'

export function findTextRangeInDoc(
  doc: ProseNode,
  needle: string,
): { from: number; to: number } | null {
  const n = needle.trim()
  if (!n) return null
  let acc = ''
  const map: number[] = []
  doc.descendants((node, pos) => {
    if (!node.isText || !node.text) return
    for (let i = 0; i < node.text.length; i++) {
      map.push(pos + i)
      acc += node.text[i]
    }
  })
  const idx = acc.indexOf(n)
  if (idx < 0) return null
  return { from: map[idx], to: map[idx + n.length - 1]! + 1 }
}

export function headingLeaf(headingChain: string): string {
  const parts = headingChain.split('/').map((s) => s.trim()).filter(Boolean)
  return parts[parts.length - 1] ?? ''
}

export function findHeadingPosInDoc(doc: ProseNode, headingChain: string): number | null {
  const leaf = headingLeaf(headingChain)
  if (!leaf) return null
  let found: number | null = null
  doc.descendants((node, pos) => {
    if (found != null) return false
    if (node.type.name.startsWith('heading') && node.textContent.trim() === leaf) {
      found = pos + 1
      return false
    }
  })
  return found
}
