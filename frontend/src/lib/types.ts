export interface TrackItem {
  id: string;           // task_id (UUID) — matches SSE ProgressUpdate.task_id
  track_uri?: string;   // Spotify URI — used for retry (populated from QueueResponse)
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

/** Shape returned by POST /api/queue and GET /api/queue. */
export interface QueueResponse {
  collection: TrackCollection;
  /** Base64 JPEG data URL, or null if the cover could not be fetched. */
  cover_data_url: string | null;
  /** Task IDs in the same order as collection.track_uris. Used as TrackItem.id. */
  task_ids: string[];
  /**
   * Current status of each task, parallel to task_ids.
   * Populated by GET /api/queue; empty array in POST /api/queue responses
   * (all newly-queued tasks start as Pending).
   */
  task_statuses: SseEvent['status'][];
  /** Human-readable status message per task (e.g. "Downloading audio…"). */
  task_messages: (string | null)[];
  /** Resolved track metadata per task (title, artists, number, duration). */
  task_track_infos: (TrackInfo | null)[];
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
