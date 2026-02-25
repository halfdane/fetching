<script lang="ts">
  import type { TrackItem } from './types';

  export let tracks: TrackItem[];

  function dotClass(status: string): string {
    switch (status) {
      case 'done':    return 'bg-green-400';
      case 'running': return 'bg-blue-400 animate-pulse';
      case 'failed':  return 'bg-red-400';
      default:        return 'bg-gray-600';
    }
  }

  function titleClass(status: string): string {
    if (status === 'failed') return 'text-red-300';
    if (status === 'done')   return 'text-gray-500';
    return 'text-gray-200';
  }
</script>

<ul class="mt-3 flex flex-col">
  {#each tracks as track (track.id)}
    <li class="flex items-center gap-3 py-1.5 text-sm border-b border-gray-800 last:border-0">

      <!-- Track number -->
      <span class="w-5 text-right text-xs text-gray-600 flex-shrink-0 tabular-nums">
        {track.number}
      </span>

      <!-- Status dot -->
      <span class="w-2 h-2 rounded-full flex-shrink-0 {dotClass(track.status)}"></span>

      <!-- Title -->
      <span class="flex-1 truncate {titleClass(track.status)}">{track.title}</span>

      <!-- Right-side indicator: mini progress bar, checkmark, or failure reason -->
      {#if track.status === 'running'}
        <div class="w-16 h-1 bg-gray-700 rounded-full flex-shrink-0">
          <div
            class="h-1 bg-blue-400 rounded-full transition-all duration-500"
            style="width: {track.progress}%"
          ></div>
        </div>
      {:else if track.status === 'done'}
        <span class="text-xs text-green-600 flex-shrink-0">✓</span>
      {:else if track.status === 'failed'}
        <span
          class="text-xs text-red-400 flex-shrink-0 max-w-48 truncate"
          title={track.failureReason}
        >{track.failureReason ?? 'failed'}</span>
      {/if}

    </li>
  {/each}
</ul>
