import { mockFetchStatus, mockSubscribeEvents } from './mock';

const IS_DEV = import.meta.env.DEV;

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
