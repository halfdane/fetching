<script lang="ts">
  import { fly, fade } from 'svelte/transition';

  interface Toast {
    id: number;
    message: string;
  }

  let { toasts }: { toasts: Toast[] } = $props();
</script>

<div class="fixed bottom-6 right-6 flex flex-col gap-2 z-50 pointer-events-none">
  {#each toasts as toast (toast.id)}
    <div
      in:fly={{ y: 16, duration: 200 }}
      out:fade={{ duration: 150 }}
      class="bg-gray-800 border border-gray-700 text-white text-sm px-4 py-3 rounded-xl shadow-xl max-w-xs"
    >
      {toast.message}
    </div>
  {/each}
</div>
