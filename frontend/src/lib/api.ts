import { mockFetchStatus, mockSubscribeEvents, mockSubscribeRawEvents, mockQueueUrl } from './mock';
import type { RawEvent, QueueResponse } from './types';

const IS_DEV = import.meta.env.DEV;

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

export async function fetchStatus(): Promise<string> {
  if (IS_DEV) return mockFetchStatus();
  const res = await fetch('/api/status');
  if (!res.ok) throw new Error('Failed to fetch status');
  return await res.text();
}

export function subscribeEvents(
  onUpdate: (data: { id: string; status: string; progress: number }) => void
): () => void {
  if (IS_DEV) return mockSubscribeEvents(onUpdate);
  const es = new EventSource('/events');
  es.onmessage = (event) => {
    try { onUpdate(JSON.parse(event.data)); } catch {}
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
