<script lang="ts">
// Mock queue data
interface QueueItem {
  id: string;
  cover: string;
  title: string;
  artist: string;
  trackCount: number;
  status: string;
  progress: number; // 0-100
}

const queue: QueueItem[] = [
  {
    id: '1',
    cover: 'https://placehold.co/80x80?text=Album',
    title: 'Mock Album',
    artist: 'Mock Artist',
    trackCount: 12,
    status: 'Fetching 5/12 tracks',
    progress: 42,
  },
  {
    id: '2',
    cover: 'https://placehold.co/80x80?text=Playlist',
    title: 'Mock Playlist',
    artist: 'Mock DJ',
    trackCount: 8,
    status: 'Queued',
    progress: 0,
  },
];
</script>

<div class="flex flex-col gap-6 mt-8">
  {#each queue as item}
    <div class="bg-gray-900 bg-opacity-70 rounded-xl p-6 shadow-lg flex items-center gap-6">
      <img src={item.cover} alt="cover" class="w-20 h-20 rounded-lg object-cover" />
      <div class="flex-1">
        <div class="flex items-center gap-2">
          <h2 class="text-xl font-bold">{item.title}</h2>
          <span class="text-sm text-gray-400">{item.artist}</span>
          <span class="ml-2 bg-gray-700 text-xs px-2 py-1 rounded">{item.trackCount} tracks</span>
        </div>
        <div class="mt-2 text-green-400">{item.status}</div>
        <div class="mt-4 h-2 bg-gray-700 rounded">
          <div class="h-2 bg-blue-500 rounded transition-all" style="width: {item.progress}%"></div>
        </div>
      </div>
    </div>
  {/each}
</div>
