export interface TrackItem {
  id: string;           // track_uri — used to match SSE events
  number: number;
  title: string;
  status: 'pending' | 'running' | 'done' | 'failed' | string;
  progress: number; // 0-100
  failureReason?: string;
}

export interface QueueItem {
  id: string;           // collection uri_str
  cover: string;
  title: string;
  artist: string;
  trackCount: number;
  status: 'pending' | 'running' | 'done' | 'failed' | string;
  progress: number; // 0-100
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
  cover_url: string;
}

export interface RawEvent {
  timestamp: string; // HH:MM:SS.mmm
  raw: string;       // verbatim JSON
}
