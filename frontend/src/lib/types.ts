export interface TrackItem {
  id: string;           // task_id (UUID) — matches SSE ProgressUpdate.task_id
  number: number;
  title: string;
  status: 'pending' | 'running' | 'done' | 'failed' | string;
  progress: number; // 0-100
  failureReason?: string;
}

export interface QueueItem {
  id: string;           // collection uri_str — used for duplicate detection
  cover: string;
  title: string;
  artist: string;
  trackCount: number;
  status: 'pending' | 'running' | 'done' | 'failed' | string;
  progress: number; // 0-100, derived from track completions
  tracks?: TrackItem[];
}

/** Mirrors the Rust TrackCollection struct (serde snake_case). */
export interface TrackCollection {
  uri_str: string;
  spotify_id: string;
  collection_type: 'Album' | 'Playlist' | 'Show' | 'SingleTrack' | 'SingleEpisode';
  title: string;
  artists: string[];
  cover_id: string | null;
  upc: string | null;
  total_tracks: number;
  popularity: number | null;
  label: string | null;
  date: string | null;
  track_uris: string[];
}

/** Shape returned by POST /api/queue on success. */
export interface QueueResponse {
  collection: TrackCollection;
  /** Base64 JPEG data URL, or null if the cover could not be fetched. */
  cover_data_url: string | null;
  /** Task IDs in the same order as collection.track_uris. Used as TrackItem.id. */
  task_ids: string[];
}

/** Shape of SSE events emitted by GET /events. Mirrors Rust ProgressUpdate. */
export interface SseEvent {
  task_id: string;
  status: { type: 'pending' | 'running' | 'done' | 'failed'; reason?: string };
  message?: string;
}

export interface RawEvent {
  timestamp: string; // HH:MM:SS.mmm
  raw: string;       // verbatim JSON
}
