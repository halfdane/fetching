<script lang="ts">
import type { QueueItem } from './types';
import TrackList from './TrackList.svelte';

let {
  queue,
  loading = false,
  error = '',
}: {
  queue: QueueItem[];
  loading?: boolean;
  error?: string;
} = $props();

let expanded: Record<string, boolean> = $state({});

function toggle(id: string) {
  expanded = { ...expanded, [id]: !expanded[id] };
}

function statusColor(status: string): string {
  switch (status) {
    case 'done':    return 'text-green-400';
    case 'running': return 'text-blue-400';
    case 'failed':  return 'text-red-400';
    default:        return 'text-gray-500';
  }
}

function barColor(status: string): string {
  switch (status) {
    case 'done':    return 'bg-green-500';
    case 'running': return 'bg-blue-500';
    case 'failed':  return 'bg-red-500';
    default:        return 'bg-gray-600';
  }
}
</script>

<div class="w-full max-w-2xl flex flex-col gap-4 mt-8">
  {#if loading}
    <p class="text-gray-500 text-center py-16">Loading…</p>
  {:else if error}
    <p class="text-red-400 text-center py-16">{error}</p>
  {:else if queue.length === 0}
    <p class="text-gray-600 text-center py-16">No downloads queued.</p>
  {:else}
    {#each queue as item (item.id)}
      <div class="bg-gray-900 bg-opacity-70 rounded-xl shadow-lg overflow-hidden">

        <!-- Clickable header -->
        <div
          class="p-5 flex items-center gap-5 cursor-pointer select-none hover:bg-white hover:bg-opacity-5 transition-colors"
          onclick={() => toggle(item.id)}
          role="button"
          tabindex="0"
          onkeydown={(e) => { if (e.key === 'Enter' || e.key === ' ') toggle(item.id); }}
        >
          <img src={item.cover} alt="cover" class="w-16 h-16 rounded-lg object-cover flex-shrink-0" />

          <div class="flex-1 min-w-0">
            <div class="flex items-center gap-2 flex-wrap">
              <h2 class="text-lg font-bold truncate">{item.title}</h2>
              <span class="text-sm text-gray-400 truncate">{item.artist}</span>
              <span class="bg-gray-700 text-xs px-2 py-0.5 rounded flex-shrink-0">{item.trackCount} tracks</span>
            </div>
            <div class="mt-1 text-sm capitalize {statusColor(item.status)}">{item.status}</div>
            <div class="mt-2 h-1.5 bg-gray-800 rounded-full">
              <div
                class="h-1.5 {barColor(item.status)} rounded-full transition-all duration-500"
                style="width: {item.progress}%"
              ></div>
            </div>
          </div>

          <!-- Expand chevron -->
          <span
            class="text-gray-600 flex-shrink-0 inline-block transition-transform duration-200 text-sm"
            class:rotate-90={expanded[item.id]}
          >&#9658;</span>
        </div>

        <!-- Expandable track list -->
        {#if expanded[item.id] && item.tracks && item.tracks.length > 0}
          <div class="border-t border-gray-800 px-5 pb-4">
            <TrackList tracks={item.tracks} />
          </div>
        {/if}

      </div>
    {/each}
  {/if}
</div>
