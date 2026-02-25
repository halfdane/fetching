/**
 * Mock data for local development (`npm run dev`).
 * Covers every visual state: done, running (animated), pending, failed.
 * Automatically excluded from production builds via import.meta.env.DEV guards in api.ts.
 */
import type { QueueItem } from './types';

// Real Spotify cover CDN URLs (stable for demo purposes)
const FLOOD_COVER =
  'https://i.scdn.co/image/ab67616d0000b273a8e671dcb8a56e92b90b6cdf';
const APOLLO_COVER =
  'https://i.scdn.co/image/ab67616d0000b273e2e352d89826aef6dbd5ff8f';
const DUMMY_COVER =
  'https://placehold.co/80x80/1a1a2e/ffffff?text=?';

export const MOCK_QUEUE: QueueItem[] = [
  {
    id: 'task-done-1',
    cover: FLOOD_COVER,
    title: 'Flood',
    artist: 'They Might Be Giants',
    trackCount: 19,
    status: 'done',
    progress: 100,
  },
  {
    id: 'task-running-1',
    cover: APOLLO_COVER,
    title: 'Apollo: Atmospheres & Soundtracks',
    artist: 'Brian Eno',
    trackCount: 11,
    status: 'running',
    progress: 35,
  },
  {
    id: 'task-pending-1',
    cover: DUMMY_COVER,
    title: 'Waiting in Queue',
    artist: 'Some Artist',
    trackCount: 8,
    status: 'pending',
    progress: 0,
  },
  {
    id: 'task-failed-1',
    cover: DUMMY_COVER,
    title: 'Failed Download',
    artist: 'Another Artist',
    trackCount: 4,
    status: 'failed',
    progress: 30,
  },
];

export function mockFetchStatus(): Promise<string> {
  return Promise.resolve('4 tasks · 1 running · 1 done · 1 failed');
}

/**
 * Simulates live SSE progress updates for the running item.
 * Progress bar bounces so animations are always visible during dev.
 * Returns a cleanup function matching the real subscribeEvents contract.
 */
export function mockSubscribeEvents(
  onUpdate: (data: { id: string; status: string; progress: number }) => void
): () => void {
  const RUNNING_ID = 'task-running-1';
  let progress = 35;
  let direction = 1;

  const interval = setInterval(() => {
    progress += direction * (3 + Math.random() * 4);
    if (progress >= 100) { progress = 100; direction = -1; }
    if (progress <= 15) { direction = 1; }
    onUpdate({ id: RUNNING_ID, status: 'running', progress: Math.round(progress) });
  }, 800);

  return () => clearInterval(interval);
}
