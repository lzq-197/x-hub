import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  folderZone,
  insertIndexForGap,
  noteAimFromFolderGap,
  noteZone,
  sameIdOrder,
  spliceForGap,
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
