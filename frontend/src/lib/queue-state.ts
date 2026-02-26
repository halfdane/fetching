import type { QueueItem, SseEvent } from './types';

/**
 * Apply a single SSE event to a queue snapshot.
 * Pure — no side effects. Returns the updated queue and whether a match was found.
 */
export function applyEvent(
  queue: QueueItem[],
  event: SseEvent,
): { result: QueueItem[]; matched: boolean } {
  let matched = false;
  const result = queue.map((item) => {
    if (!item.tracks) return item;
    const trackIdx = item.tracks.findIndex((t) => t.id === event.task_id);
    if (trackIdx === -1) return item;
    matched = true;

    const newStatus = event.status.type;
    const newProgress =
      newStatus === 'done' ? 100
      : newStatus === 'running' || newStatus === 'retrying' ? item.tracks[trackIdx].progress
      : 0;
    const failureReason =
      newStatus === 'failed' ? (event.status.reason ?? 'Unknown error') : undefined;

    const updatedTracks = item.tracks.map((t, i) => {
      if (i !== trackIdx) return t;
      const infoUpdate = event.track_info
        ? {
            title: event.track_info.title,
            artists: event.track_info.artists,
            number: event.track_info.number ?? t.number,
            duration_ms: event.track_info.duration_ms,
          }
        : {};
      const msgUpdate = event.message !== undefined ? { statusMessage: event.message } : {};
      return { ...t, status: newStatus, progress: newProgress, failureReason, ...infoUpdate, ...msgUpdate };
    });

    const anyRunning = updatedTracks.some((t) => t.status === 'running' || t.status === 'retrying');
    const anyFailed  = updatedTracks.some((t) => t.status === 'failed');
    const allDone    = updatedTracks.every((t) => t.status === 'done');
    const collectionStatus = anyRunning ? 'running'
      : allDone   ? 'done'
      : anyFailed ? 'failed'
      : 'pending';
    const collectionProgress = Math.round(
      updatedTracks.reduce((sum, t) => sum + t.progress, 0) / updatedTracks.length,
    );

    return { ...item, tracks: updatedTracks, status: collectionStatus, progress: collectionProgress };
  });
  return { result, matched };
}

/**
 * Apply an SSE event to the queue. If no matching track exists yet, the event
 * is parked in the buffer keyed by task_id for later replay.
 */
export function handleEvent(
  queue: QueueItem[],
  event: SseEvent,
  buffer: Map<string, SseEvent[]>,
): QueueItem[] {
  const { result, matched } = applyEvent(queue, event);
  if (matched) return result;
  const bucket = buffer.get(event.task_id) ?? [];
  bucket.push(event);
  buffer.set(event.task_id, bucket);
  return queue;
}

/**
 * Replay any buffered events for tracks now present in the queue.
 * Mutates the buffer (removes replayed entries). Returns the updated queue.
 * Call this immediately after adding or replacing items in the queue.
 */
export function drainBuffer(
  queue: QueueItem[],
  buffer: Map<string, SseEvent[]>,
): QueueItem[] {
  if (buffer.size === 0) return queue;
  let current = queue;
  for (const item of current) {
    for (const track of item.tracks ?? []) {
      const events = buffer.get(track.id);
      if (events?.length) {
        buffer.delete(track.id);
        for (const ev of events) {
          const { result } = applyEvent(current, ev);
          current = result;
        }
      }
    }
  }
  return current;
}
