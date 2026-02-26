import { mockFetchStatus, mockSubscribeEvents, mockSubscribeRawEvents, mockQueueUrl } from './mock';
import type { RawEvent, QueueItem, QueueResponse, SseEvent } from './types';

const IS_DEV = import.meta.env.DEV;

export function responseToQueueItem(res: QueueResponse): QueueItem {
  const { collection, cover_data_url, task_ids, task_statuses } = res;
  const statuses = task_statuses ?? [];

  const anyRunning = statuses.some(s => s.type === 'running' || s.type === 'retrying');
  const anyFailed  = statuses.some(s => s.type === 'failed');
  const allDone    = statuses.length > 0 && statuses.every(s => s.type === 'done');
  const collectionStatus = anyRunning ? 'running'
    : allDone   ? 'done'
    : anyFailed ? 'failed'
    : 'pending';
  const collectionProgress = statuses.length
    ? Math.round(statuses.filter(s => s.type === 'done').length / statuses.length * 100)
    : 0;

  return {
    id: collection.uri_str,
    cover: cover_data_url ?? '',
    title: collection.title,
    artist: collection.artists[0] ?? '',
    trackCount: collection.total_tracks,
    status: collectionStatus,
    progress: collectionProgress,
    tracks: task_ids.map((taskId, i) => {
      const s = statuses[i];
      return {
        id: taskId,
        track_uri: collection.track_uris[i],
        number: i + 1,
        title: `Track ${i + 1}`,
        status: s?.type ?? 'pending',
        progress: s?.type === 'done' ? 100 : 0,
        failureReason: s?.type === 'failed' ? s.reason : undefined,
      };
    }),
  };
}

export async function queueUrl(url: string): Promise<QueueResponse> {
  if (IS_DEV) return mockQueueUrl(url);
  const res = await fetch('/api/queue', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ url }),
  });
  if (!res.ok) throw new Error(`Server responded ${res.status}`);
  return res.json() as Promise<QueueResponse>;
}

export async function fetchQueue(): Promise<QueueResponse[]> {
  if (IS_DEV) return [];
  const res = await fetch('/api/queue');
  if (!res.ok) throw new Error(`Server responded ${res.status}`);
  return res.json() as Promise<QueueResponse[]>;
}

export async function fetchStatus(): Promise<string> {
  if (IS_DEV) return mockFetchStatus();
  const res = await fetch('/api/status');
  if (!res.ok) throw new Error('Failed to fetch status');
  return await res.text();
}

export function subscribeEvents(
  onUpdate: (event: SseEvent) => void
): () => void {
  if (IS_DEV) return mockSubscribeEvents(onUpdate);
  const es = new EventSource('/events');
  es.onmessage = (msg) => {
    try { onUpdate(JSON.parse(msg.data)); } catch {}
  };
  return () => es.close();
}

/**
 * Subscribe to every raw SSE event verbatim — including events the main UI
 * doesn't act on (session token refreshes, audio key timings, CDN chunks, etc.).
 * Intended for the developer drawer only.
 */
export function subscribeRawEvents(
  onEvent: (event: RawEvent) => void
): () => void {
  if (IS_DEV) return mockSubscribeRawEvents(onEvent);
  const es = new EventSource('/events');
  es.onmessage = (msg) => {
    onEvent({
      timestamp: new Date().toISOString().slice(11, 23),
      raw: msg.data,
    });
  };
  return () => es.close();
}
