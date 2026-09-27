import type { Node as ProseNode } from '@milkdown/kit/prose/model'

/** Map index for an artificial whitespace separator (not a doc position). */
const SEP = -1

/**
 * Build a search haystack: text nodes joined with a single space when the
 * parent block changes (so multi-paragraph chunks match plainForMatch needles).
 * `map[i]` is the doc position of haystack char i, or SEP for inserted spaces.
 */
export function buildSearchIndex(doc: ProseNode): { plain: string; map: number[] } {
  let plain = ''
  const map: number[] = []
  let lastParent: ProseNode | null | undefined = undefined

  doc.descendants((node, pos, parent) => {
    if (!node.isText || !node.text) return
    if (lastParent !== undefined && parent !== lastParent && plain.length > 0) {
      plain += ' '
      map.push(SEP)
    }
    lastParent = parent
    for (let i = 0; i < node.text.length; i++) {
      map.push(pos + i)
      plain += node.text[i]
    }
  })

  return { plain, map }
}

function rangeFromMatch(map: number[], idx: number, len: number): { from: number; to: number } | null {
  let from: number | null = null
  let to: number | null = null
  for (let i = idx; i < idx + len; i++) {
    const p = map[i]
    if (p == null || p < 0) continue
    if (from == null) from = p
    to = p + 1
  }
  if (from == null || to == null) return null
  return { from, to }
}

/**
 * Find `needle` in the doc. Optional `preferNearPos` picks the match whose
 * `from` is closest to that position (design §5.2.4).
 */
export function findTextRangeInDoc(
  doc: ProseNode,
  needle: string,
  preferNearPos?: number | null,
): { from: number; to: number } | null {
  const n = needle.trim()
  if (!n) return null
  const { plain, map } = buildSearchIndex(doc)
  if (!plain) return null

  let best: { from: number; to: number } | null = null
  let bestDist = Infinity
  let searchFrom = 0
  while (searchFrom <= plain.length) {
    const idx = plain.indexOf(n, searchFrom)
    if (idx < 0) break
    const range = rangeFromMatch(map, idx, n.length)
    if (range) {
      if (preferNearPos == null || preferNearPos === undefined) {
        return range
      }
      const dist = Math.abs(range.from - preferNearPos)
      if (dist < bestDist) {
        best = range
        bestDist = dist
      }
    }
    searchFrom = idx + 1
  }
  return best
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
