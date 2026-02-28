// ---------------------------------------------------------------------------
// UI-level types (used by Svelte components)
// ---------------------------------------------------------------------------

export interface TrackItem {
  id: string;           // track row id (UUID)
  task_id: string;      // task_id (UUID) — matches SSE ProgressUpdate.task_id
  track_uri: string;    // Spotify URI
  number: number;
  title: string;
  artists?: string[];
  duration_ms?: number;
  status: 'pending' | 'running' | 'done' | 'failed' | string;
  progress: number; // 0-100, kept for collection-level progress roll-up
  /** Human-readable status message updated at each download stage. */
  statusMessage?: string;
  failureReason?: string;
}

export interface QueueItem {
  id: string;           // collection id (UUID)
  uri: string;          // Spotify URI (e.g. spotify:album:…)
  cover: string;        // cover URL or empty string
  title: string;
  artist: string;
  trackCount: number;
  status: 'pending' | 'running' | 'done' | 'failed' | string;
  progress: number; // 0-100, derived from track completions
  tracks: TrackItem[];
  registered_at: string; // ISO 8601 — used for display ordering (newest first)
}

// ---------------------------------------------------------------------------
// Server response types (match Rust serde JSON output)
// ---------------------------------------------------------------------------

/** Row from GET /api/collections (SQL aggregate, mirrors Rust CollectionRow). */
export interface CollectionRow {
  id: string;
  uri: string;
  collection_type: string;
  title: string;
  artists: string[];
  cover_id: string | null;
  date: string | null;
  total_tracks: number;
  /** Pre-aggregated: "pending" | "running" | "done" | "failed" */
  status: string;
  /** 0–100 */
  progress: number;
  registered_at: string;
}

/** Row from GET /api/collections/:id/tracks (mirrors Rust TrackRow). */
export interface TrackRow {
  id: string;
  uri: string;
  title: string | null;
  artists: string[] | null;
  number: number | null;
  disc_number: number | null;
  duration_ms: number | null;
  task_id: string;
  /** "pending" | "running" | "retrying" | "done" | "failed:reason" */
  status: string;
  message: string | null;
}

/** Response from POST /api/queue. */
export interface PostQueueResponse {
  collection_id: string;
  track_ids: string[];
  task_ids: string[];
}

/** Resolved track metadata sent in the first `running` SSE event. */
export interface TrackInfo {
  title: string;
  artists: string[];
  number?: number;
  disc_number?: number;
  duration_ms: number;
}

/** Shape of SSE events emitted by GET /events. Mirrors Rust ProgressUpdate. */
export interface SseEvent {
  task_id: string;
  collection_id: string;
  status: {
    type: 'pending' | 'running' | 'retrying' | 'done' | 'failed';
    reason?: string;
  };
  message?: string;
  /** Present on the first `running` update, once Spotify metadata is resolved. */
  track_info?: TrackInfo;
}

export interface RawEvent {
  timestamp: string; // HH:MM:SS.mmm
  raw: string;       // verbatim JSON
}
