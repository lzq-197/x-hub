/** UTF-8 byte length of a JS string */
export function utf8ByteLength(s: string): number {
  return new TextEncoder().encode(s).length
}

/** Slice `s` by UTF-8 byte offsets [start, end). Clamps; empty if invalid. */
export function utf8ByteSlice(s: string, start: number, end: number): string {
  if (!Number.isFinite(start) || !Number.isFinite(end) || start < 0 || end <= start) return ''
  const bytes = new TextEncoder().encode(s)
  if (start >= bytes.length) return ''
  const lo = Math.min(start, bytes.length)
  const hi = Math.min(end, bytes.length)
  if (lo >= hi) return ''
  return new TextDecoder('utf-8', { fatal: false }).decode(bytes.subarray(lo, hi))
}

export function offsetsValid(
  start: number | null | undefined,
  end: number | null | undefined,
  byteLen: number,
): boolean {
  return (
    typeof start === 'number' &&
    typeof end === 'number' &&
    start >= 0 &&
    end > start &&
    end <= byteLen
  )
}

/** Enough strip for Crepe textContent match; not a full Markdown parser. */
export function plainForMatch(mdSlice: string): string {
  let s = mdSlice
  s = s.replace(/!\[([^\]]*)\]\([^)]*\)/g, '$1')
  s = s.replace(/\[([^\]]+)\]\([^)]*\)/g, '$1')
  s = s.replace(/(\*\*|__|~~)(.*?)\1/g, '$2')
  s = s.replace(/([*_])(.*?)\1/g, '$2')
  s = s.replace(/`([^`]+)`/g, '$1')
  return s.replace(/\s+/g, ' ').trim()
}
