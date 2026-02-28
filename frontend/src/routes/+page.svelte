<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import QueueView from '../lib/QueueView.svelte';
  import DevDrawer from '../lib/DevDrawer.svelte';
  import AddToQueue from '../lib/AddToQueue.svelte';
  import Toast from '../lib/Toast.svelte';
  import { fetchStatus, subscribeEvents, queueUrl, fetchCollections, fetchCollectionTracks, collectionToQueueItem } from '../lib/api';
  import { MOCK_QUEUE } from '../lib/mock';
  import type { QueueItem, SseEvent } from '../lib/types';

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

  // --- Load initial state ---

  /** Fetch all collections and their tracks from the REST API. */
  async function loadFullQueue(): Promise<QueueItem[]> {
    const collections = await fetchCollections();
    const items: QueueItem[] = [];
    for (const col of collections) {
      const tracks = await fetchCollectionTracks(col.id);
      items.push(collectionToQueueItem(col, tracks));
    }
    return items;
  }

  // --- Targeted SSE patching ---

  /** Apply a single SSE event to the queue in-place (no re-fetch). */
  function applySseUpdate(event: SseEvent) {
    const collectionIdx = queue.findIndex(q => q.id === event.collection_id);
    if (collectionIdx === -1) {
      // Unknown collection — could be newly queued from another client.
      // Schedule a full refresh to pick it up.
      scheduleRefresh();
      return;
    }

    const item = queue[collectionIdx];
    const trackIdx = item.tracks.findIndex(t => t.task_id === event.task_id);
    if (trackIdx === -1) {
      // Unknown task in a known collection — refresh that collection.
      scheduleCollectionRefresh(event.collection_id);
      return;
    }

    // Patch the track in-place
    const track = { ...item.tracks[trackIdx] };
    track.status = event.status.type;
    track.statusMessage = event.message ?? track.statusMessage;
    track.progress = event.status.type === 'done' ? 100 : track.progress;
    if (event.status.type === 'failed') {
      track.failureReason = event.status.reason;
    }
    if (event.track_info) {
      track.title = event.track_info.title;
      track.artists = event.track_info.artists;
      track.number = event.track_info.number ?? track.number;
      track.duration_ms = event.track_info.duration_ms;
    }

    const newTracks = [...item.tracks];
    newTracks[trackIdx] = track;

    // Recompute collection-level status and progress
    const doneCount = newTracks.filter(t => t.status === 'done').length;
    const runningCount = newTracks.filter(t => t.status === 'running' || t.status === 'retrying').length;
    const failedCount = newTracks.filter(t => t.status === 'failed').length;
    const progress = newTracks.length > 0 ? Math.round((doneCount / newTracks.length) * 100) : 0;
    const status = runningCount > 0 ? 'running'
      : doneCount === newTracks.length ? 'done'
      : failedCount > 0 ? 'failed'
      : 'pending';

    const newItem = { ...item, tracks: newTracks, progress, status };
    const newQueue = [...queue];
    newQueue[collectionIdx] = newItem;
    queue = newQueue;
  }

  /** Debounced full refresh (fallback when SSE references unknown collection). */
  let debounceTimer: ReturnType<typeof setTimeout> | undefined;
  function scheduleRefresh() {
    clearTimeout(debounceTimer);
    debounceTimer = setTimeout(async () => {
      try { queue = await loadFullQueue(); } catch (e) { console.warn('Refresh failed:', e); }
    }, 500);
  }

  /** Refresh a single collection's tracks from the server. */
  async function scheduleCollectionRefresh(collectionId: string) {
    try {
      const collections = await fetchCollections();
      const col = collections.find(c => c.id === collectionId);
      if (!col) return;
      const tracks = await fetchCollectionTracks(collectionId);
      const updated = collectionToQueueItem(col, tracks);
      // Remove old entry (if any), add updated, then sort newest-first.
      const filtered = queue.filter(q => q.id !== collectionId);
      filtered.push(updated);
      filtered.sort((a, b) => b.registered_at.localeCompare(a.registered_at));
      queue = filtered;
    } catch (e) {
      console.warn('Collection refresh failed:', e);
    }
  }

  onMount(async () => {
    window.addEventListener('beforeinstallprompt', (e) => {
      e.preventDefault();
      installPrompt = e;
    });
    try {
      await fetchStatus();

      // Hydrate from server.
      if (!import.meta.env.DEV) {
        queue = await loadFullQueue();
      } else {
        queue = [...MOCK_QUEUE];
      }

      // SSE: targeted in-place patching, with full refresh on reconnect.
      unsubscribe = subscribeEvents(applySseUpdate, async () => {
        try { queue = await loadFullQueue(); } catch (e) { console.warn('Reconnect refresh failed:', e); }
      });

      // Handle Web Share Target — Spotify appends ?url=... when sharing to this PWA
      const params = new URLSearchParams(window.location.search);
      const sharedText = params.get('url') ?? params.get('text') ?? '';
      const spotifyUrl = sharedText.match(/https:\/\/open\.spotify\.com\/\S+/)?.[0];
      if (spotifyUrl) {
        history.replaceState({}, '', '/');
        try {
          const postRes = await queueUrl(spotifyUrl);
          await scheduleCollectionRefresh(postRes.collection_id);
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

  async function handleQueued(collectionId: string) {
    // Always refresh — even if the collection already exists (re-add resets it).
    await scheduleCollectionRefresh(collectionId);
    const item = queue.find(q => q.id === collectionId);
    if (item) {
      const noun = item.trackCount === 1 ? 'track' : 'tracks';
      addToast(`Added '${item.title}' (${item.trackCount} ${noun})`);
    }
  }

  async function handleRetry(uri: string) {
    try {
      const res = await queueUrl(uri);
      await scheduleCollectionRefresh(res.collection_id);
      const item = queue.find(q => q.id === res.collection_id);
      if (item) {
        const noun = item.trackCount === 1 ? 'track' : 'tracks';
        addToast(`Re-queued '${item.title}' (${item.trackCount} ${noun})`);
      }
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
