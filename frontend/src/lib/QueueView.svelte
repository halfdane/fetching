<script lang="ts">
import { onMount } from 'svelte';
import { fetchStatus, subscribeEvents } from './api';

interface QueueItem {
  id: string;
  cover: string;
  title: string;
  artist: string;
  trackCount: number;
  status: string;
  progress: number; // 0-100
}

let queue: QueueItem[] = [];
let loading = true;
let error = '';

onMount(async () => {
  try {
    loading = true;
    // Fetch initial status (replace with real parsing as needed)
    const status = await fetchStatus();
    // For now, just show a single dummy item with backend status
    queue = [{
      id: 'backend',
      cover: 'https://placehold.co/80x80?text=Backend',
      title: 'Backend Status',
      artist: '',
      trackCount: 1,
      status,
      progress: 0,
    }];
    // Subscribe to SSE for progress updates
    subscribeEvents((update) => {
      // Update queue with progress info (replace with real logic)
      queue[0].status = update.status;
      queue[0].progress = update.progress || 0;
    });
    loading = false;
  } catch (e) {
    error = e.message || 'Failed to load backend';
    loading = false;
  }
});
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
