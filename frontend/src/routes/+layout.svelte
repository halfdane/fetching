<script lang="ts">
import { onMount } from 'svelte';
import '../app.css';

let { children } = $props();

onMount(() => {
  if ('serviceWorker' in navigator) {
    navigator.serviceWorker.register('/sw.js').catch(() => {});
  }
});
</script>

<svelte:head>
	<link rel="icon" type="image/png" href="/icon-512.png" />
</svelte:head>

{@render children()}
