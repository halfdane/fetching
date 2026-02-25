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

  function handleQueued(item: QueueItem) {
    if (queue.some((q) => q.id === item.id)) {
      // Already queued — surface a brief visual hint by re-expanding that card
      // (the queue list itself doesn't need to change)
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
