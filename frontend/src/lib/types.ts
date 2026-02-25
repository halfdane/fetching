export interface TrackItem {
  id: string;
  number: number;
  title: string;
  status: 'pending' | 'running' | 'done' | 'failed' | string;
  progress: number; // 0-100
  failureReason?: string;
}

export interface QueueItem {
  id: string;
  cover: string;
  title: string;
  artist: string;
  trackCount: number;
  status: 'pending' | 'running' | 'done' | 'failed' | string;
  progress: number; // 0-100
  tracks?: TrackItem[];
}

export interface RawEvent {
  timestamp: string; // HH:MM:SS.mmm
  raw: string;       // verbatim JSON
}
