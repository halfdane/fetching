<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { subscribeRawEvents } from './api';
  import type { RawEvent } from './types';

  const MAX_EVENTS = 200;

  let open = false;
  let events: RawEvent[] = [];
  let unsubscribe: (() => void) | undefined;

  onMount(() => {
    unsubscribe = subscribeRawEvents((e) => {
      // Prepend so newest is always at the top; cap to avoid unbounded growth.
      events = [e, ...events.slice(0, MAX_EVENTS - 1)];
    });
  });

  onDestroy(() => unsubscribe?.());

  function toggle() { open = !open; }
</script>

<div class="fixed bottom-0 right-4 z-50 flex flex-col items-end">

  <!-- Slide-up panel -->
  <div
    class="w-96 bg-gray-950 border border-gray-700 border-b-0 rounded-t-xl shadow-2xl overflow-hidden transition-all duration-300 ease-in-out"
    style="max-height: {open ? '24rem' : '0'}; opacity: {open ? 1 : 0};"
  >
    <!-- Panel header -->
    <div class="px-3 py-2 border-b border-gray-800 flex items-center justify-between sticky top-0 bg-gray-950">
      <span class="text-xs text-gray-400 font-mono">raw event log</span>
      <span class="text-xs text-gray-600 tabular-nums">{events.length} events</span>
    </div>

    <!-- Event list (newest first) -->
    <div class="overflow-y-auto font-mono text-xs" style="max-height: calc(24rem - 2.25rem);">
      {#if events.length === 0}
        <p class="text-gray-600 italic p-3">Waiting for events…</p>
      {:else}
        {#each events as event (event.timestamp + event.raw.slice(0, 20))}
          <div class="px-3 py-1 flex gap-2 border-b border-gray-900 hover:bg-gray-900">
            <span class="text-gray-600 flex-shrink-0 tabular-nums">{event.timestamp}</span>
            <span class="text-green-300 break-all">{event.raw}</span>
          </div>
        {/each}
      {/if}
    </div>
  </div>

  <!-- Toggle button — always visible -->
  <button
    onclick={toggle}
    class="bg-gray-800 hover:bg-gray-700 border border-gray-600 rounded-t-lg px-4 py-1.5 text-xs font-mono text-gray-400 hover:text-white transition-colors flex items-center gap-1.5"
    title="Developer event log (raw SSE)"
  >
    <span>&lt;/&gt;</span>
    <span class="text-gray-600 text-xs">{open ? '▾' : '▴'}</span>
  </button>

</div>
