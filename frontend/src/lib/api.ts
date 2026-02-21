// API helpers for backend integration
export async function fetchStatus() {
  const res = await fetch('/api/status');
  if (!res.ok) throw new Error('Failed to fetch status');
  return await res.text();
}

export function subscribeEvents(onUpdate: (data: any) => void) {
  const eventSource = new EventSource('/events');
  eventSource.onmessage = (event) => {
    try {
      const data = JSON.parse(event.data);
      onUpdate(data);
    } catch {}
  };
  return () => eventSource.close();
}
