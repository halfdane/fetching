<script lang="ts">
  import type { TrackItem } from './types';

  let { tracks }: { tracks: TrackItem[] } = $props();

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

  function statusLabel(track: TrackItem): string | null {
    if (track.status === 'failed') return null; // handled separately
    if (track.statusMessage) return track.statusMessage;
    if (track.status === 'running') return 'Running\u2026';
    return null;
  }

  function statusLabelClass(status: string): string {
    if (status === 'done') return 'text-green-600';
    if (status === 'running') return 'text-blue-400';
    return 'text-gray-500';
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

      <!-- Right-side indicator: status message (persists after completion), or failure reason -->
      {#if track.status === 'failed'}
        <span
          class="text-xs text-red-400 flex-shrink-0 max-w-48 truncate"
          title={track.failureReason}
        >{track.failureReason ?? 'Failed'}</span>
      {:else}
        {@const label = statusLabel(track)}
        {#if label}
          <span class="text-xs flex-shrink-0 max-w-48 truncate {statusLabelClass(track.status)}">{label}</span>
        {/if}
      {/if}

    </li>
  {/each}
</ul>
