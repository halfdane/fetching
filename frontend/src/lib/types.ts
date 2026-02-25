export interface QueueItem {
  id: string;
  cover: string;
  title: string;
  artist: string;
  trackCount: number;
  status: 'pending' | 'running' | 'done' | 'failed' | string;
  progress: number; // 0-100
}
