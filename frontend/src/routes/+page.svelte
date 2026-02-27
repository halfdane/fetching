<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import QueueView from '../lib/QueueView.svelte';
  import DevDrawer from '../lib/DevDrawer.svelte';
  import AddToQueue from '../lib/AddToQueue.svelte';
  import Toast from '../lib/Toast.svelte';
  import { fetchStatus, subscribeSseSignal, queueUrl, responseToQueueItem, fetchQueue } from '../lib/api';
  import { MOCK_QUEUE } from '../lib/mock';
  import type { QueueItem } from '../lib/types';

  let queue = $state<QueueItem[]>([]);
  let loading = $state(true);
  let error = $state('');
  let toasts = $state<{ id: number; message: string }[]>([]);
  let toastSeq = 0;
  let unsubscribe: (() => void) | undefined;

  // PWA install prompt
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  let installPrompt = $state<any>(null);
  const isStandalone = typeof window !== 'undefined' &&
    (window.matchMedia('(display-mode: standalone)').matches ||
     ('standalone' in navigator && (navigator as any).standalone === true));

  function install() {
    if (!installPrompt) return;
    installPrompt.prompt();
    installPrompt.userChoice.then(() => { installPrompt = null; });
  }

  function addToast(message: string) {
    const id = toastSeq++;
    toasts = [...toasts, { id, message }];
    setTimeout(() => { toasts = toasts.filter((t) => t.id !== id); }, 3000);
  }

  // --- Backend is the single source of truth ---

  /** Fetch from GET /api/queue and replace the entire queue state. */
  async function refreshQueue() {
    try {
      const snapshot = await fetchQueue();
      queue = snapshot.map(responseToQueueItem);
    } catch (e: unknown) {
      console.warn('Failed to refresh queue:', e);
    }
  }

  /** Debounced refresh: coalesces rapid SSE bursts into a single fetch. */
  let debounceTimer: ReturnType<typeof setTimeout> | undefined;
  function scheduleRefresh() {
    clearTimeout(debounceTimer);
    debounceTimer = setTimeout(refreshQueue, 300);
  }

  onMount(async () => {
    window.addEventListener('beforeinstallprompt', (e) => {
      e.preventDefault();
      installPrompt = e;
    });
    try {
      await fetchStatus();

      // Hydrate from server snapshot.
      if (!import.meta.env.DEV) {
        const snapshot = await fetchQueue();
        queue = snapshot.map(responseToQueueItem);
      } else {
        queue = [...MOCK_QUEUE];
      }

      // SSE: every event triggers a debounced re-fetch.
      unsubscribe = subscribeSseSignal(scheduleRefresh);

      // Handle Web Share Target — Spotify appends ?url=... when sharing to this PWA
      const params = new URLSearchParams(window.location.search);
      const sharedText = params.get('url') ?? params.get('text') ?? '';
      const spotifyUrl = sharedText.match(/https:\/\/open\.spotify\.com\/\S+/)?.[0];
      if (spotifyUrl) {
        history.replaceState({}, '', '/');
        try {
          await queueUrl(spotifyUrl);
          await refreshQueue();
          addToast('Queued shared URL');
        } catch (e: unknown) {
          addToast(`Failed to queue shared URL: ${e instanceof Error ? e.message : e}`);
        }
      }
      loading = false;
    } catch (e: unknown) {
      error = e instanceof Error ? e.message : 'Failed to load backend';
      loading = false;
    }
  });

  onDestroy(() => {
    unsubscribe?.();
    clearTimeout(debounceTimer);
  });

  async function handleQueued(item: QueueItem) {
    if (queue.some((q) => q.id === item.id)) {
      return;
    }
    // Optimistic add so the card appears instantly, then reconcile with server.
    queue = [...queue, item];
    const noun = item.trackCount === 1 ? 'track' : 'tracks';
    addToast(`Added '${item.title}' (${item.trackCount} ${noun})`);
    await refreshQueue();
  }

  async function handleRetry(uri: string) {
    try {
      const res = await queueUrl(uri);
      const item = responseToQueueItem(res);
      const noun = item.trackCount === 1 ? 'track' : 'tracks';
      addToast(`Re-queued '${item.title}' (${item.trackCount} ${noun})`);
      await refreshQueue();
    } catch (e: unknown) {
      addToast(`Retry failed: ${e instanceof Error ? e.message : e}`);
    }
  }
</script>

<main class="min-h-screen flex flex-col items-center bg-gradient-to-br from-black via-gray-900 to-gray-800 text-white px-4">
  <h1 class="text-4xl font-bold mt-12 mb-8">Fetching</h1>
  {#if installPrompt && !isStandalone}
    <button
      onclick={install}
      class="mb-4 flex items-center gap-2 px-4 py-2 rounded-lg bg-blue-600 hover:bg-blue-500 text-sm font-medium transition-colors"
    >
      <svg xmlns="http://www.w3.org/2000/svg" class="w-4 h-4" viewBox="0 0 20 20" fill="currentColor">
        <path fill-rule="evenodd" d="M3 16a1 1 0 001 1h12a1 1 0 000-2H4a1 1 0 00-1 1zm10.293-8.707a1 1 0 011.414 1.414l-4 4a1 1 0 01-1.414 0l-4-4a1 1 0 111.414-1.414L9 10.586V3a1 1 0 112 0v7.586l2.293-2.293z" clip-rule="evenodd" />
      </svg>
      Install app
    </button>
  {/if}
  <AddToQueue onQueued={handleQueued} />
  <QueueView {queue} {loading} {error} onRetry={handleRetry} />
</main>

<DevDrawer />
<Toast {toasts} />
