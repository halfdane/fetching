/**
 * Mock data for local development (`npm run dev`).
 * Covers every visual state: done, running (animated), pending, failed.
 * Automatically excluded from production builds via import.meta.env.DEV guards in api.ts.
 */
import type { QueueItem, TrackItem, RawEvent, QueueResponse } from './types';

// Real Spotify cover CDN URLs (stable for demo purposes)
const FLOOD_COVER =
  'https://i.scdn.co/image/ab67616d0000b273a8e671dcb8a56e92b90b6cdf';
const APOLLO_COVER =
  'https://i.scdn.co/image/ab67616d0000b273e2e352d89826aef6dbd5ff8f';
const DUMMY_COVER =
  'https://placehold.co/80x80/1a1a2e/ffffff?text=?';

// ---------------------------------------------------------------------------
// Track lists
// ---------------------------------------------------------------------------

const FLOOD_TRACKS: TrackItem[] = [
  { id: 'fl-01', number: 1,  title: 'Theme from Flood',                  status: 'done',    progress: 100 },
  { id: 'fl-02', number: 2,  title: 'Birdhouse in Your Soul',            status: 'done',    progress: 100 },
  { id: 'fl-03', number: 3,  title: 'Lucky Ball and Chain',              status: 'done',    progress: 100 },
  { id: 'fl-04', number: 4,  title: 'Istanbul (Not Constantinople)',     status: 'done',    progress: 100 },
  { id: 'fl-05', number: 5,  title: 'Dead',                             status: 'done',    progress: 100 },
  { id: 'fl-06', number: 6,  title: 'Your Racist Friend',               status: 'done',    progress: 100 },
  { id: 'fl-07', number: 7,  title: 'Particle Man',                     status: 'done',    progress: 100 },
  { id: 'fl-08', number: 8,  title: 'Twisting',                         status: 'done',    progress: 100 },
  { id: 'fl-09', number: 9,  title: 'We Want a Rock',                   status: 'done',    progress: 100 },
  { id: 'fl-10', number: 10, title: 'Someone Keeps Moving My Chair',    status: 'done',    progress: 100 },
  { id: 'fl-11', number: 11, title: 'Hearing Aid',                      status: 'done',    progress: 100 },
  { id: 'fl-12', number: 12, title: 'Minimum Wage',                     status: 'done',    progress: 100 },
  { id: 'fl-13', number: 13, title: 'Letterbox',                        status: 'done',    progress: 100 },
  { id: 'fl-14', number: 14, title: 'Whistling in the Dark',            status: 'done',    progress: 100 },
  { id: 'fl-15', number: 15, title: 'Hot Cha',                          status: 'done',    progress: 100 },
  { id: 'fl-16', number: 16, title: 'Women & Men',                      status: 'done',    progress: 100 },
  { id: 'fl-17', number: 17, title: 'Sapphire Bullets of Pure Love',    status: 'done',    progress: 100 },
  { id: 'fl-18', number: 18, title: 'They Might Be Giants',             status: 'done',    progress: 100 },
  { id: 'fl-19', number: 19, title: 'Road Movie to Berlin',             status: 'done',    progress: 100 },
];

const APOLLO_TRACKS: TrackItem[] = [
  { id: 'ap-01', number: 1,  title: 'The Overview',      status: 'done',    progress: 100 },
  { id: 'ap-02', number: 2,  title: 'Weightless',         status: 'done',    progress: 100 },
  { id: 'ap-03', number: 3,  title: 'Always Returning',   status: 'running', progress: 62  },
  { id: 'ap-04', number: 4,  title: 'Drift',              status: 'pending', progress: 0   },
  { id: 'ap-05', number: 5,  title: 'Silver Morning',     status: 'pending', progress: 0   },
  { id: 'ap-06', number: 6,  title: 'For This Moment',    status: 'pending', progress: 0   },
  { id: 'ap-07', number: 7,  title: 'Deep Blue Day',      status: 'pending', progress: 0   },
  { id: 'ap-08', number: 8,  title: 'Sparrowfall (1)',    status: 'pending', progress: 0   },
  { id: 'ap-09', number: 9,  title: 'Sparrowfall (2)',    status: 'pending', progress: 0   },
  { id: 'ap-10', number: 10, title: 'Sparrowfall (3)',    status: 'pending', progress: 0   },
  { id: 'ap-11', number: 11, title: 'Landing',            status: 'pending', progress: 0   },
];

const PENDING_TRACKS: TrackItem[] = Array.from({ length: 8 }, (_, i) => ({
  id: `pe-${String(i + 1).padStart(2, '0')}`,
  number: i + 1,
  title: `Track ${String(i + 1).padStart(2, '0')}`,
  status: 'pending',
  progress: 0,
}));

const FAILED_TRACKS: TrackItem[] = [
  { id: 'fa-01', number: 1, title: 'Lost Signal',       status: 'done',   progress: 100 },
  { id: 'fa-02', number: 2, title: 'Broken Frequency',  status: 'done',   progress: 100 },
  { id: 'fa-03', number: 3, title: 'Static Noise',      status: 'failed', progress: 45,
    failureReason: 'Audio key error after 4 attempts' },
  { id: 'fa-04', number: 4, title: 'Distant Echo',      status: 'failed', progress: 0,
    failureReason: 'Track not available in your region' },
];

// ---------------------------------------------------------------------------
// Queue
// ---------------------------------------------------------------------------

export const MOCK_QUEUE: QueueItem[] = [
  {
    id: 'task-done-1',
    cover: FLOOD_COVER,
    title: 'Flood',
    artist: 'They Might Be Giants',
    trackCount: 19,
    status: 'done',
    progress: 100,
    tracks: FLOOD_TRACKS,
  },
  {
    id: 'task-running-1',
    cover: APOLLO_COVER,
    title: 'Apollo: Atmospheres & Soundtracks',
    artist: 'Brian Eno',
    trackCount: 11,
    status: 'running',
    progress: 35,
    tracks: APOLLO_TRACKS,
  },
  {
    id: 'task-pending-1',
    cover: DUMMY_COVER,
    title: 'Waiting in Queue',
    artist: 'Some Artist',
    trackCount: 8,
    status: 'pending',
    progress: 0,
    tracks: PENDING_TRACKS,
  },
  {
    id: 'task-failed-1',
    cover: DUMMY_COVER,
    title: 'Failed Download',
    artist: 'Another Artist',
    trackCount: 4,
    status: 'failed',
    progress: 50,
    tracks: FAILED_TRACKS,
  },
];

export function mockFetchStatus(): Promise<string> {
  return Promise.resolve('4 tasks · 1 running · 1 done · 1 pending · 1 failed');
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

// ---------------------------------------------------------------------------
// Mock POST /api/queue response
// ---------------------------------------------------------------------------

const KIND_OF_BLUE: QueueResponse = {
  collection: {
    uri_str: 'spotify:album:1weenld61qoidwYuZ1GESA',
    spotify_id: '1weenld61qoidwYuZ1GESA',
    collection_type: 'Album',
    title: 'Kind of Blue',
    artists: ['Miles Davis'],
    cover_id: 'bef23e01c90f66c785a6f7771weenld61qoidwYuZ1',
    total_tracks: 5,
    track_uris: [
      'spotify:track:7q3kkfAVpmcZ8g6JUThi3o',
      'spotify:track:1YGpSCgGVi2LJz67TBw4pc',
      'spotify:track:6vLDzbqCp6l7tObdVrPQYe',
      'spotify:track:5FHfmFhqLfJaJDJmFjzQFV',
      'spotify:track:7BGlUWOzXKwA2Gf7TzOhFJ',
    ],
    upc: '074646403723',
    popularity: 85,
    label: 'Columbia',
    date: '1959-08-17',
  },
  cover_url: 'https://i.scdn.co/image/ab67616d0000b273e2e352d89826aef6dbd5ff8f',
};

/** Returns a fake QueueResponse for any URL entered in dev. */
export function mockQueueUrl(_url: string): Promise<QueueResponse> {
  return new Promise((resolve) =>
    setTimeout(() => resolve(KIND_OF_BLUE), 600)
  );
}

// ---------------------------------------------------------------------------
// Raw event mock — emits everything, including "noise" the UI ignores
// ---------------------------------------------------------------------------

function randomHex(n: number): string {
  return Array.from({ length: n }, () => Math.floor(Math.random() * 16).toString(16)).join('');
}

const RAW_TEMPLATES: Array<() => object> = [
  () => ({
    type: 'track_progress',
    task_id: 'task-running-1',
    track_uri: 'spotify:track:' + randomHex(22),
    track_title: 'Always Returning',
    status: 'running',
    bytes_downloaded: Math.floor(Math.random() * 6_000_000 + 500_000),
    bytes_total: 8_419_769,
  }),
  () => ({
    type: 'session',
    event: 'token_check',
    account: 'halfdane1',
    expires_in_s: Math.floor(Math.random() * 3600),
    action: 'none',
  }),
  () => ({
    type: 'audio_key',
    track_uri: 'spotify:track:' + randomHex(22),
    attempt: 1,
    result: 'ok',
    latency_ms: Math.floor(Math.random() * 200 + 50),
  }),
  () => ({
    type: 'cdn_chunk',
    track_uri: 'spotify:track:' + randomHex(22),
    seq: Math.floor(Math.random() * 128),
    chunk_bytes: 65536,
    cdn_node: 'audio-ak-spotify-com.akamaized.net',
  }),
  () => ({
    type: 'queue_internal',
    event: 'worker_woken',
    pending_count: Math.floor(Math.random() * 8),
    semaphore_available: 1,
  }),
  () => ({
    type: 'session',
    event: 'token_refresh',
    account: 'halfdane1',
    expires_in_s: 3600,
    status: 'ok',
  }),
];

export function mockSubscribeRawEvents(
  onEvent: (event: RawEvent) => void
): () => void {
  let i = 0;
  const interval = setInterval(() => {
    const payload = RAW_TEMPLATES[i % RAW_TEMPLATES.length]();
    onEvent({
      timestamp: new Date().toISOString().slice(11, 23),
      raw: JSON.stringify(payload),
    });
    i++;
  }, 1200);
  return () => clearInterval(interval);
}
