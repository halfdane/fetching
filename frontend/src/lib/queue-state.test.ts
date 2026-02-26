import { describe, it, expect, beforeEach } from 'vitest';
import { applyEvent, handleEvent, drainBuffer } from './queue-state';
import type { QueueItem, SseEvent, TrackItem } from './types';

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function makeTrack(id: string, status = 'pending', progress = 0): TrackItem {
  return { id, number: 1, title: 'Track', status, progress };
}

function makeItem(id: string, tracks: QueueItem['tracks']): QueueItem {
  return { id, cover: '', title: 'Album', artist: 'Artist', trackCount: tracks!.length, status: 'pending', progress: 0, tracks };
}

function event(task_id: string, type: string, extra?: Partial<SseEvent>): SseEvent {
  return { task_id, status: { type }, ...extra } as SseEvent;
}

// ---------------------------------------------------------------------------
// applyEvent
// ---------------------------------------------------------------------------

describe('applyEvent', () => {
  it('returns matched=false for unknown task_id', () => {
    const q = [makeItem('a', [makeTrack('t1')])];
    const { matched } = applyEvent(q, event('unknown', 'running'));
    expect(matched).toBe(false);
  });

  it('marks track running and sets collection status', () => {
    const q = [makeItem('a', [makeTrack('t1'), makeTrack('t2')])];
    const { result, matched } = applyEvent(q, event('t1', 'running'));
    expect(matched).toBe(true);
    expect(result[0].tracks![0].status).toBe('running');
    expect(result[0].status).toBe('running');
  });

  it('sets progress=100 when track is done', () => {
    const q = [makeItem('a', [makeTrack('t1', 'running', 50)])];
    const { result } = applyEvent(q, event('t1', 'done'));
    expect(result[0].tracks![0].progress).toBe(100);
  });

  it('derives collection done only when all tracks done', () => {
    const q = [makeItem('a', [makeTrack('t1', 'done', 100), makeTrack('t2', 'pending', 0)])];
    const { result } = applyEvent(q, event('t2', 'done'));
    expect(result[0].status).toBe('done');
    expect(result[0].progress).toBe(100);
  });

  it('derives collection failed when any track fails', () => {
    const q = [makeItem('a', [makeTrack('t1', 'done', 100), makeTrack('t2', 'running', 50)])];
    const { result } = applyEvent(q, { task_id: 't2', status: { type: 'failed', reason: 'timeout' } } as SseEvent);
    expect(result[0].status).toBe('failed');
    expect(result[0].tracks![1].failureReason).toBe('timeout');
  });

  it('updates track metadata from track_info', () => {
    const q = [makeItem('a', [makeTrack('t1')])];
    const { result } = applyEvent(q, {
      task_id: 't1',
      status: { type: 'running' },
      track_info: { title: 'Blue in Green', artists: ['Miles Davis'], number: 3, duration_ms: 327000 },
    } as SseEvent);
    expect(result[0].tracks![0].title).toBe('Blue in Green');
    expect(result[0].tracks![0].artists).toEqual(['Miles Davis']);
    expect(result[0].tracks![0].number).toBe(3);
  });

  it('leaves unrelated items untouched', () => {
    const q = [makeItem('a', [makeTrack('t1')]), makeItem('b', [makeTrack('t2')])];
    const { result } = applyEvent(q, event('t1', 'done'));
    expect(result[1]).toBe(q[1]); // same reference — not cloned
  });
});

// ---------------------------------------------------------------------------
// handleEvent + drainBuffer
// ---------------------------------------------------------------------------

describe('handleEvent', () => {
  let buffer: Map<string, SseEvent[]>;
  beforeEach(() => { buffer = new Map(); });

  it('applies event immediately when track exists', () => {
    const q = [makeItem('a', [makeTrack('t1')])];
    const next = handleEvent(q, event('t1', 'running'), buffer);
    expect(next[0].tracks![0].status).toBe('running');
    expect(buffer.size).toBe(0);
  });

  it('parks event in buffer when track does not exist yet', () => {
    const q: QueueItem[] = [];
    const next = handleEvent(q, event('t1', 'running'), buffer);
    expect(next).toBe(q); // queue unchanged
    expect(buffer.get('t1')).toHaveLength(1);
  });

  it('accumulates multiple events for the same unknown track', () => {
    const q: QueueItem[] = [];
    handleEvent(q, event('t1', 'running'), buffer);
    handleEvent(q, event('t1', 'done'), buffer);
    expect(buffer.get('t1')).toHaveLength(2);
  });
});

describe('drainBuffer', () => {
  let buffer: Map<string, SseEvent[]>;
  beforeEach(() => { buffer = new Map(); });

  it('returns same queue reference when buffer is empty', () => {
    const q = [makeItem('a', [makeTrack('t1')])];
    expect(drainBuffer(q, buffer)).toBe(q);
  });

  it('replays buffered events once item is added (retry race)', () => {
    const q: QueueItem[] = [];
    // Events arrive before queueUrl resolves
    handleEvent(q, event('t1', 'running'), buffer);
    handleEvent(q, event('t1', 'done'), buffer);
    // queueUrl resolves — item added
    const withItem = [...q, makeItem('album', [makeTrack('t1')])];
    const drained = drainBuffer(withItem, buffer);
    expect(drained[0].tracks![0].status).toBe('done');
    expect(drained[0].tracks![0].progress).toBe(100);
    expect(buffer.size).toBe(0); // cleaned up
  });

  it('replays events in order', () => {
    const statuses: string[] = [];
    const q: QueueItem[] = [];
    handleEvent(q, event('t1', 'pending'), buffer);
    handleEvent(q, event('t1', 'running'), buffer);
    const withItem = [makeItem('album', [makeTrack('t1')])];
    const drained = drainBuffer(withItem, buffer);
    // Final status should be the last event applied
    expect(drained[0].tracks![0].status).toBe('running');
  });

  it('leaves events for still-unknown tracks in the buffer', () => {
    handleEvent([], event('unknown-track', 'running'), buffer);
    const q = [makeItem('album', [makeTrack('t1')])];
    drainBuffer(q, buffer);
    expect(buffer.get('unknown-track')).toHaveLength(1);
  });
});
