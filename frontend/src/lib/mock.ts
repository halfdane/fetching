/**
 * Mock data for local development (`npm run dev`).
 * Covers every visual state: done, running (animated), pending, failed.
 * Automatically excluded from production builds via import.meta.env.DEV guards in api.ts.
 */
import type { QueueItem, TrackItem, RawEvent, PostQueueResponse, CollectionRow, TrackRow, SseEvent } from './types';

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
  { id: 'fl-t01', task_id: 'fl-01', track_uri: 'spotify:track:fl-01', number: 1,  title: 'Theme from Flood',                  status: 'done',    progress: 100, statusMessage: 'Downloaded' },
  { id: 'fl-t02', task_id: 'fl-02', track_uri: 'spotify:track:fl-02', number: 2,  title: 'Birdhouse in Your Soul',            status: 'done',    progress: 100, statusMessage: 'Downloaded' },
  { id: 'fl-t03', task_id: 'fl-03', track_uri: 'spotify:track:fl-03', number: 3,  title: 'Lucky Ball and Chain',              status: 'done',    progress: 100, statusMessage: 'Downloaded' },
  { id: 'fl-t04', task_id: 'fl-04', track_uri: 'spotify:track:fl-04', number: 4,  title: 'Istanbul (Not Constantinople)',     status: 'done',    progress: 100, statusMessage: 'File already exists' },
  { id: 'fl-t05', task_id: 'fl-05', track_uri: 'spotify:track:fl-05', number: 5,  title: 'Dead',                             status: 'done',    progress: 100, statusMessage: 'Downloaded' },
  { id: 'fl-t06', task_id: 'fl-06', track_uri: 'spotify:track:fl-06', number: 6,  title: 'Your Racist Friend',               status: 'done',    progress: 100, statusMessage: 'File already exists' },
  { id: 'fl-t07', task_id: 'fl-07', track_uri: 'spotify:track:fl-07', number: 7,  title: 'Particle Man',                     status: 'done',    progress: 100, statusMessage: 'Downloaded' },
  { id: 'fl-t08', task_id: 'fl-08', track_uri: 'spotify:track:fl-08', number: 8,  title: 'Twisting',                         status: 'done',    progress: 100, statusMessage: 'Downloaded' },
  { id: 'fl-t09', task_id: 'fl-09', track_uri: 'spotify:track:fl-09', number: 9,  title: 'We Want a Rock',                   status: 'done',    progress: 100, statusMessage: 'Downloaded' },
  { id: 'fl-t10', task_id: 'fl-10', track_uri: 'spotify:track:fl-10', number: 10, title: 'Someone Keeps Moving My Chair',    status: 'done',    progress: 100, statusMessage: 'Downloaded' },
  { id: 'fl-t11', task_id: 'fl-11', track_uri: 'spotify:track:fl-11', number: 11, title: 'Hearing Aid',                      status: 'done',    progress: 100, statusMessage: 'File already exists' },
  { id: 'fl-t12', task_id: 'fl-12', track_uri: 'spotify:track:fl-12', number: 12, title: 'Minimum Wage',                     status: 'done',    progress: 100, statusMessage: 'Downloaded' },
  { id: 'fl-t13', task_id: 'fl-13', track_uri: 'spotify:track:fl-13', number: 13, title: 'Letterbox',                        status: 'done',    progress: 100, statusMessage: 'Downloaded' },
  { id: 'fl-t14', task_id: 'fl-14', track_uri: 'spotify:track:fl-14', number: 14, title: 'Whistling in the Dark',            status: 'done',    progress: 100, statusMessage: 'Downloaded' },
  { id: 'fl-t15', task_id: 'fl-15', track_uri: 'spotify:track:fl-15', number: 15, title: 'Hot Cha',                          status: 'done',    progress: 100, statusMessage: 'Downloaded' },
  { id: 'fl-t16', task_id: 'fl-16', track_uri: 'spotify:track:fl-16', number: 16, title: 'Women & Men',                      status: 'done',    progress: 100, statusMessage: 'Downloaded' },
  { id: 'fl-t17', task_id: 'fl-17', track_uri: 'spotify:track:fl-17', number: 17, title: 'Sapphire Bullets of Pure Love',    status: 'done',    progress: 100, statusMessage: 'Downloaded' },
  { id: 'fl-t18', task_id: 'fl-18', track_uri: 'spotify:track:fl-18', number: 18, title: 'They Might Be Giants',             status: 'done',    progress: 100, statusMessage: 'Downloaded' },
  { id: 'fl-t19', task_id: 'fl-19', track_uri: 'spotify:track:fl-19', number: 19, title: 'Road Movie to Berlin',             status: 'done',    progress: 100, statusMessage: 'Downloaded' },
];

const APOLLO_TRACKS: TrackItem[] = [
  { id: 'ap-t01', task_id: 'ap-01', track_uri: 'spotify:track:ap-01', number: 1,  title: 'The Overview',      status: 'done',    progress: 100, statusMessage: 'Downloaded' },
  { id: 'ap-t02', task_id: 'ap-02', track_uri: 'spotify:track:ap-02', number: 2,  title: 'Weightless',         status: 'done',    progress: 100, statusMessage: 'File already exists' },
  { id: 'ap-t03', task_id: 'ap-03', track_uri: 'spotify:track:ap-03', number: 3,  title: 'Always Returning',   status: 'running', progress: 0,   statusMessage: 'Fetching cover art\u2026' },
  { id: 'ap-t04', task_id: 'ap-04', track_uri: 'spotify:track:ap-04', number: 4,  title: 'Drift',              status: 'pending', progress: 0 },
  { id: 'ap-t05', task_id: 'ap-05', track_uri: 'spotify:track:ap-05', number: 5,  title: 'Silver Morning',     status: 'pending', progress: 0 },
  { id: 'ap-t06', task_id: 'ap-06', track_uri: 'spotify:track:ap-06', number: 6,  title: 'For This Moment',    status: 'pending', progress: 0 },
  { id: 'ap-t07', task_id: 'ap-07', track_uri: 'spotify:track:ap-07', number: 7,  title: 'Deep Blue Day',      status: 'pending', progress: 0 },
  { id: 'ap-t08', task_id: 'ap-08', track_uri: 'spotify:track:ap-08', number: 8,  title: 'Sparrowfall (1)',    status: 'pending', progress: 0 },
  { id: 'ap-t09', task_id: 'ap-09', track_uri: 'spotify:track:ap-09', number: 9,  title: 'Sparrowfall (2)',    status: 'pending', progress: 0 },
  { id: 'ap-t10', task_id: 'ap-10', track_uri: 'spotify:track:ap-10', number: 10, title: 'Sparrowfall (3)',    status: 'pending', progress: 0 },
  { id: 'ap-t11', task_id: 'ap-11', track_uri: 'spotify:track:ap-11', number: 11, title: 'Landing',            status: 'pending', progress: 0 },
];

const PENDING_TRACKS: TrackItem[] = Array.from({ length: 8 }, (_, i) => ({
  id: `pe-t${String(i + 1).padStart(2, '0')}`,
  task_id: `pe-${String(i + 1).padStart(2, '0')}`,
  track_uri: `spotify:track:pe-${String(i + 1).padStart(2, '0')}`,
  number: i + 1,
  title: `Track ${String(i + 1).padStart(2, '0')}`,
  status: 'pending',
  progress: 0,
}));

const FAILED_TRACKS: TrackItem[] = [
  { id: 'fa-t01', task_id: 'fa-01', track_uri: 'spotify:track:fa-01', number: 1, title: 'Lost Signal',       status: 'done',   progress: 100, statusMessage: 'Downloaded' },
  { id: 'fa-t02', task_id: 'fa-02', track_uri: 'spotify:track:fa-02', number: 2, title: 'Broken Frequency',  status: 'done',   progress: 100, statusMessage: 'File already exists' },
  { id: 'fa-t03', task_id: 'fa-03', track_uri: 'spotify:track:fa-03', number: 3, title: 'Static Noise',      status: 'failed', progress: 45,
    failureReason: 'Audio key error after 4 attempts' },
  { id: 'fa-t04', task_id: 'fa-04', track_uri: 'spotify:track:fa-04', number: 4, title: 'Distant Echo',      status: 'failed', progress: 0,
    failureReason: 'Track not available in your region' },
];

// ---------------------------------------------------------------------------
// Queue
// ---------------------------------------------------------------------------

export const MOCK_QUEUE: QueueItem[] = [
  {
    id: 'col-done-1',
    uri: 'spotify:album:flood',
    cover: FLOOD_COVER,
    title: 'Flood',
    artist: 'They Might Be Giants',
    trackCount: 19,
    status: 'done',
    progress: 100,
    tracks: FLOOD_TRACKS,
  },
  {
    id: 'col-running-1',
    uri: 'spotify:album:apollo',
    cover: APOLLO_COVER,
    title: 'Apollo: Atmospheres & Soundtracks',
    artist: 'Brian Eno',
    trackCount: 11,
    status: 'running',
    progress: 35,
    tracks: APOLLO_TRACKS,
  },
  {
    id: 'col-pending-1',
    uri: 'spotify:album:waiting',
    cover: DUMMY_COVER,
    title: 'Waiting in Queue',
    artist: 'Some Artist',
    trackCount: 8,
    status: 'pending',
    progress: 0,
    tracks: PENDING_TRACKS,
  },
  {
    id: 'col-failed-1',
    uri: 'spotify:album:failed',
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
  return Promise.resolve('4 collections · 1 running · 1 done · 1 pending · 1 failed');
}

/** Mock GET /api/collections. */
export function mockFetchCollections(): Promise<CollectionRow[]> {
  return Promise.resolve([]);
}

/** Mock GET /api/collections/:id/tracks. */
export function mockFetchCollectionTracks(_id: string): Promise<TrackRow[]> {
  return Promise.resolve([]);
}

/**
 * Simulates live SSE progress updates for the running item.
 * Advances Apollo tracks one-by-one so the progress bar animates during dev.
 * Returns a cleanup function matching the real subscribeEvents contract.
 */
export function mockSubscribeEvents(
  onUpdate: (event: SseEvent) => void
): () => void {
  // ap-03 is the currently-running track; cycle through ap-03 … ap-11
  const taskIds = ['ap-03', 'ap-04', 'ap-05', 'ap-06', 'ap-07', 'ap-08', 'ap-09', 'ap-10', 'ap-11'];
  const stages = ['Fetching cover art\u2026', 'Downloading audio\u2026', 'Writing tags\u2026'];
  let idx = 0;
  let stageIdx = 0;

  // Immediately mark the first track as running with a stage message
  onUpdate({ task_id: taskIds[0], collection_id: 'col-running-1', status: { type: 'running' }, message: stages[0] });

  // Advance stage messages within a track, then move to the next track
  const interval = setInterval(() => {
    stageIdx++;
    if (stageIdx < stages.length) {
      // Still in the same track — emit next stage
      onUpdate({ task_id: taskIds[idx], collection_id: 'col-running-1', status: { type: 'running' }, message: stages[stageIdx] });
    } else {
      // Finish current track, start next
      stageIdx = 0;
      const doneId = taskIds[idx];
      idx = (idx + 1) % taskIds.length;
      const nextId = taskIds[idx];
      onUpdate({ task_id: doneId,  collection_id: 'col-running-1', status: { type: 'done' },    message: 'Downloaded' });
      onUpdate({ task_id: nextId, collection_id: 'col-running-1', status: { type: 'running' }, message: stages[0] });
    }
  }, 800);

  return () => clearInterval(interval);
}

// ---------------------------------------------------------------------------
// Mock POST /api/queue response
// ---------------------------------------------------------------------------

const KIND_OF_BLUE: PostQueueResponse = {
  collection_id: 'col-kob-1',
  track_ids: ['kob-t01', 'kob-t02', 'kob-t03', 'kob-t04', 'kob-t05'],
  task_ids: ['kob-01', 'kob-02', 'kob-03', 'kob-04', 'kob-05'],
};

/** Returns a fake PostQueueResponse for any URL entered in dev. */
export function mockQueueUrl(_url: string): Promise<PostQueueResponse> {
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
