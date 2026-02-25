<script lang="ts">
  import { queueUrl } from './api';
  import type { QueueItem, QueueResponse } from './types';

  let { onQueued }: { onQueued: (item: QueueItem) => void } = $props();

  let url = $state('');
  let loading = $state(false);
  let error = $state('');

  function responseToQueueItem(res: QueueResponse): QueueItem {
    const { collection, cover_data_url, task_ids } = res;
    return {
      id: collection.uri_str,
      cover: cover_data_url ?? '',
      title: collection.title,
      artist: collection.artists[0] ?? '',
      trackCount: collection.total_tracks,
      status: 'pending',
      progress: 0,
      tracks: task_ids.map((taskId, i) => ({
        id: taskId,
        number: i + 1,
        title: `Track ${i + 1}`,
        status: 'pending',
        progress: 0,
      })),
    };
  }

  async function submit() {
    const trimmed = url.trim();
    if (!trimmed) return;
    error = '';
    loading = true;
    try {
      const response = await queueUrl(trimmed);
      url = '';
      onQueued(responseToQueueItem(response));
    } catch (e: unknown) {
      error = e instanceof Error ? e.message : 'Failed to queue URL';
    } finally {
      loading = false;
    }
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key === 'Enter') submit();
  }
</script>

<div class="w-full max-w-2xl mt-4">
  <div class="flex gap-2">
    <input
      type="text"
      bind:value={url}
      onkeydown={onKeydown}
      placeholder="spotify:album:… or spotify:track:…"
      disabled={loading}
      class="flex-1 bg-gray-900 border border-gray-700 rounded-lg px-4 py-2.5 text-sm text-white
             placeholder-gray-600 focus:outline-none focus:border-gray-500 focus:ring-1 focus:ring-gray-500
             disabled:opacity-50 transition-colors font-mono"
    />
    <button
      onclick={submit}
      disabled={loading || !url.trim()}
      class="px-5 py-2.5 rounded-lg text-sm font-semibold transition-colors flex-shrink-0
             bg-blue-600 hover:bg-blue-500 text-white
             disabled:opacity-40 disabled:cursor-not-allowed"
    >
      {loading ? 'Adding…' : 'Add'}
    </button>
  </div>
  {#if error}
    <p class="mt-2 text-sm text-red-400">{error}</p>
  {/if}
</div>
