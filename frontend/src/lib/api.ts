import { mockFetchStatus, mockSubscribeEvents, mockSubscribeRawEvents, mockQueueUrl, mockFetchCollections, mockFetchCollectionTracks } from './mock';
import type { RawEvent, QueueItem, TrackItem, CollectionRow, TrackRow, PostQueueResponse, SseEvent } from './types';

const IS_DEV = import.meta.env.DEV;

// ---------------------------------------------------------------------------
// Conversion helpers
// ---------------------------------------------------------------------------

/** Parse the status string from the DB into {status, failureReason}. */
function parseTaskStatus(raw: string): { status: string; failureReason?: string } {
  if (raw.startsWith('failed:')) {
    return { status: 'failed', failureReason: raw.slice('failed:'.length) };
  }
  return { status: raw };
}

/** Convert a CollectionRow + optional TrackRow[] into a UI QueueItem. */
export function collectionToQueueItem(row: CollectionRow, trackRows?: TrackRow[]): QueueItem {
  const tracks: TrackItem[] = trackRows
    ? trackRows.map((t, i) => trackRowToTrackItem(t, i))
    : [];

  return {
    id: row.id,
    uri: row.uri,
    cover: '',
    title: row.title,
    artist: row.artists[0] ?? '',
    trackCount: row.total_tracks,
    status: row.status,
    progress: row.progress,
    tracks,
    registered_at: row.registered_at,
  };
}

/** Convert a TrackRow into a UI TrackItem. */
export function trackRowToTrackItem(row: TrackRow, index: number): TrackItem {
  const { status, failureReason } = parseTaskStatus(row.status);

  return {
    id: row.id,
    task_id: row.task_id,
    track_uri: row.uri,
    number: row.number ?? (index + 1),
    title: row.title ?? `Track ${index + 1}`,
    artists: row.artists ?? undefined,
    duration_ms: row.duration_ms ?? undefined,
    status,
    progress: status === 'done' ? 100 : 0,
    statusMessage: row.message ?? undefined,
    failureReason,
  };
}

// ---------------------------------------------------------------------------
// API functions
// ---------------------------------------------------------------------------

/** POST /api/queue — enqueue a Spotify URL, returns IDs only. */
export async function queueUrl(url: string): Promise<PostQueueResponse> {
  if (IS_DEV) return mockQueueUrl(url);
  const res = await fetch('/api/queue', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ url }),
  });
  if (!res.ok) throw new Error(`Server responded ${res.status}`);
  return res.json() as Promise<PostQueueResponse>;
}

/** GET /api/collections — list all collections with aggregate status. */
export async function fetchCollections(): Promise<CollectionRow[]> {
  if (IS_DEV) return mockFetchCollections();
  const res = await fetch('/api/collections');
  if (!res.ok) throw new Error(`Server responded ${res.status}`);
  return res.json() as Promise<CollectionRow[]>;
}

/** GET /api/collections/:id/tracks — tracks + task status for one collection. */
export async function fetchCollectionTracks(collectionId: string): Promise<TrackRow[]> {
  if (IS_DEV) return mockFetchCollectionTracks(collectionId);
  const res = await fetch(`/api/collections/${encodeURIComponent(collectionId)}/tracks`);
  if (!res.ok) throw new Error(`Server responded ${res.status}`);
  return res.json() as Promise<TrackRow[]>;
}

export async function fetchStatus(): Promise<string> {
  if (IS_DEV) return mockFetchStatus();
  const res = await fetch('/api/status');
  if (!res.ok) throw new Error('Failed to fetch status');
  return await res.text();
}

/** Subscribe to typed SSE events for targeted in-place patching. */
export function subscribeEvents(
  onUpdate: (event: SseEvent) => void,
  onReconnect?: () => void,
): () => void {
  if (IS_DEV) return mockSubscribeEvents(onUpdate);
  const es = new EventSource('/events');
  let opened = false;
  es.onopen = () => {
    if (opened && onReconnect) onReconnect();
    opened = true;
  };
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
