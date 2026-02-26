<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import QueueView from '../lib/QueueView.svelte';
  import DevDrawer from '../lib/DevDrawer.svelte';
  import AddToQueue from '../lib/AddToQueue.svelte';
  import Toast from '../lib/Toast.svelte';
  import { fetchStatus, subscribeEvents, queueUrl, responseToQueueItem, fetchQueue } from '../lib/api';
  import { handleEvent, drainBuffer } from '../lib/queue-state';
  import { MOCK_QUEUE } from '../lib/mock';
  import type { QueueItem, SseEvent } from '../lib/types';

  let queue = $state<QueueItem[]>([]);
  let loading = $state(true);
  let error = $state('');
  let toasts = $state<{ id: number; message: string }[]>([]);
  let toastSeq = 0;
  let unsubscribe: (() => void) | undefined;

  // Buffer for SSE events that arrive before their queue item exists (retry race).
  // Keyed by task_id — see queue-state.ts.
  const buffer = new Map<string, SseEvent[]>();

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

  onMount(async () => {
    window.addEventListener('beforeinstallprompt', (e) => {
      e.preventDefault();
      installPrompt = e;
    });
    try {
      await fetchStatus();

      // Subscribe first so SSE events are buffered while we fetch the snapshot.
      unsubscribe = subscribeEvents((event) => {
        queue = handleEvent(queue, event, buffer);
      });

      // Hydrate from server snapshot, then drain any SSE events that raced in.
      if (!import.meta.env.DEV) {
        const snapshot = await fetchQueue();
        queue = snapshot.map(responseToQueueItem);
        queue = drainBuffer(queue, buffer);
      } else {
        queue = [...MOCK_QUEUE];
      }

      // Handle Web Share Target — Spotify appends ?url=... when sharing to this PWA
      const params = new URLSearchParams(window.location.search);
      const sharedText = params.get('url') ?? params.get('text') ?? '';
      const spotifyUrl = sharedText.match(/https:\/\/open\.spotify\.com\/\S+/)?.[0];
      if (spotifyUrl) {
        history.replaceState({}, '', '/');
        try {
          const res = await queueUrl(spotifyUrl);
          handleQueued(responseToQueueItem(res));
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

  onDestroy(() => unsubscribe?.());

  function handleQueued(item: QueueItem) {
    if (queue.some((q) => q.id === item.id)) {
      return;
    }
    queue = [...queue, item];
    queue = drainBuffer(queue, buffer);
    const noun = item.trackCount === 1 ? 'track' : 'tracks';
    addToast(`Added '${item.title}' (${item.trackCount} ${noun})`);
  }

  async function handleRetry(uri: string) {
    try {
      const res = await queueUrl(uri);
      const item = responseToQueueItem(res);
      // In-place replace: same collection uri_str, fresh track UUIDs.
      // The item slides from "failed" to "pending" via animate:flip.
      queue = queue.map((q) => q.id === uri ? item : q);
      queue = drainBuffer(queue, buffer);
      const noun = item.trackCount === 1 ? 'track' : 'tracks';
      addToast(`Re-queued '${item.title}' (${item.trackCount} ${noun})`);
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
