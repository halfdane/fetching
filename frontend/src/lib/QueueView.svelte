<script lang="ts">
import { onMount, onDestroy } from 'svelte';
import { fetchStatus, subscribeEvents } from './api';
import { MOCK_QUEUE } from './mock';
import type { QueueItem } from './types';

let queue: QueueItem[] = [];
let loading = true;
let error = '';
let unsubscribe: (() => void) | undefined;

onMount(async () => {
  try {
    loading = true;
    await fetchStatus();
    queue = import.meta.env.DEV ? [...MOCK_QUEUE] : [];
    unsubscribe = subscribeEvents((update) => {
      queue = queue.map((item) =>
        item.id === update.id
          ? { ...item, status: update.status, progress: update.progress }
          : item
      );
    });
    loading = false;
  } catch (e: unknown) {
    error = e instanceof Error ? e.message : 'Failed to load backend';
    loading = false;
  }
});

onDestroy(() => unsubscribe?.());
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
