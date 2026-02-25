<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import QueueView from '../lib/QueueView.svelte';
  import DevDrawer from '../lib/DevDrawer.svelte';
  import AddToQueue from '../lib/AddToQueue.svelte';
  import { fetchStatus, subscribeEvents } from '../lib/api';
  import { MOCK_QUEUE } from '../lib/mock';
  import type { QueueItem } from '../lib/types';

  let queue = $state<QueueItem[]>([]);
  let loading = $state(true);
  let error = $state('');
  let unsubscribe: (() => void) | undefined;

  onMount(async () => {
    try {
      await fetchStatus();
      queue = import.meta.env.DEV ? [...MOCK_QUEUE] : [];
      unsubscribe = subscribeEvents((event) => {
        queue = queue.map((item) => {
          if (!item.tracks) return item;
          const trackIdx = item.tracks.findIndex((t) => t.id === event.task_id);
          if (trackIdx === -1) return item;

          const newStatus = event.status.type;
          const newProgress =
            newStatus === 'done' ? 100
            : newStatus === 'running' ? item.tracks[trackIdx].progress
            : 0;
          const failureReason =
            newStatus === 'failed' ? (event.status.reason ?? 'Unknown error') : undefined;

          const updatedTracks = item.tracks.map((t, i) =>
            i === trackIdx
              ? { ...t, status: newStatus, progress: newProgress, failureReason }
              : t
          );

          // Derive collection-level status and progress from individual tracks
          const anyRunning = updatedTracks.some((t) => t.status === 'running');
          const anyFailed  = updatedTracks.some((t) => t.status === 'failed');
          const allDone    = updatedTracks.every((t) => t.status === 'done');
          const collectionStatus = anyRunning ? 'running'
            : allDone    ? 'done'
            : anyFailed  ? 'failed'
            : 'pending';
          const collectionProgress = Math.round(
            updatedTracks.reduce((sum, t) => sum + t.progress, 0) / updatedTracks.length
          );

          return { ...item, tracks: updatedTracks, status: collectionStatus, progress: collectionProgress };
        });
      });
      loading = false;
    } catch (e: unknown) {
      error = e instanceof Error ? e.message : 'Failed to load backend';
      loading = false;
    }
  });

  onDestroy(() => unsubscribe?.());

  function handleQueued(item: QueueItem) {
    if (queue.some((q) => q.id === item.id)) {
      return;
    }
    queue = [...queue, item];
  }
</script>

<main class="min-h-screen flex flex-col items-center bg-gradient-to-br from-black via-gray-900 to-gray-800 text-white px-4">
  <h1 class="text-4xl font-bold mt-12 mb-8">Fetching</h1>
  <AddToQueue onQueued={handleQueued} />
  <QueueView {queue} {loading} {error} />
</main>

<DevDrawer />
