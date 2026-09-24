import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  folderZone,
  insertIndexForGap,
  noteAimFromFolderGap,
  noteZone,
  sameIdOrder,
  siblingNoteIdsInFolder,
  spliceForGap,
  spliceNoteGapInFullSiblings,
} from '../src/utils/noteTreeHit.ts'

describe('noteTreeHit zones', () => {
  it('folderZone splits 30/40/30', () => {
    assert.equal(folderZone(10, 0, 100), 'above')
    assert.equal(folderZone(29, 0, 100), 'above')
    assert.equal(folderZone(50, 0, 100), 'nest')
    assert.equal(folderZone(71, 0, 100), 'below')
    assert.equal(folderZone(99, 0, 100), 'below')
  })

  it('noteZone is half-half', () => {
    assert.equal(noteZone(10, 0, 100), 'above')
    assert.equal(noteZone(50, 0, 100), 'below')
  })
})

describe('noteTreeHit splice', () => {
  it('reorders between siblings', () => {
    assert.deepEqual(spliceForGap([1, 2, 3], 3, 1, 2), [1, 3, 2])
    assert.deepEqual(spliceForGap([1, 2, 3], 1, 2, 3), [2, 1, 3])
  })

  it('gap against self stays put', () => {
    // below self (before=self, after=next)
    assert.deepEqual(spliceForGap([1, 2, 3], 2, 2, 3), [1, 2, 3])
    // above self (before=prev, after=self)
    assert.deepEqual(spliceForGap([1, 2, 3], 2, 1, 2), [1, 2, 3])
  })

  it('insert at ends', () => {
    assert.equal(insertIndexForGap([1, 2, 3], 9, null, 1), 0)
    assert.equal(insertIndexForGap([1, 2, 3], 9, 3, null), 3)
    assert.deepEqual(spliceForGap([1, 2, 3], 9, null, 1), [9, 1, 2, 3])
    assert.deepEqual(spliceForGap([1, 2, 3], 9, 3, null), [1, 2, 3, 9])
  })

  it('sameIdOrder', () => {
    assert.equal(sameIdOrder([1, 2], [1, 2]), true)
    assert.equal(sameIdOrder([1, 2], [2, 1]), false)
  })

  it('noteAimFromFolderGap nests into folder above', () => {
    assert.deepEqual(noteAimFromFolderGap(7), { kind: 'nest', folderId: 7 })
    assert.equal(noteAimFromFolderGap(null), null)
  })
})

describe('siblingNoteIdsInFolder (unfiltered peers)', () => {
  const notes = [
    { id: 1, folder_id: 10, sort_order: 0 },
    { id: 2, folder_id: 10, sort_order: 1 }, // tag-filtered out in UI
    { id: 3, folder_id: 10, sort_order: 2 },
    { id: 4, folder_id: 20, sort_order: 0 },
    { id: 5, folder_id: null, sort_order: 0 },
  ]

  it('includes peers that a tag filter would hide', () => {
    const full = siblingNoteIdsInFolder(notes, 10)
    assert.deepEqual(full, [1, 2, 3])
    // visible-only subset must NOT be what reorderNotes receives
    const visibleOnly = [1, 3]
    assert.notDeepEqual(full, visibleOnly)
  })

  it('filters by folder_id only (null = top-level)', () => {
    assert.deepEqual(siblingNoteIdsInFolder(notes, null), [5])
    assert.deepEqual(siblingNoteIdsInFolder(notes, 20), [4])
  })

  it('merges filtered gap into full sibling list without dropping peers', () => {
    const full = siblingNoteIdsInFolder(notes, 10)
    // UI shows [1, 3]; user drops 3 above 1 → before=null, after=1
    const next = spliceNoteGapInFullSiblings(full, 3, null, 1)
    assert.deepEqual(next, [3, 1, 2])
    assert.ok(next.includes(2), 'hidden peer 2 must remain in reorder payload')
  })
})
