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
	<!-- SVG favicon: stable URL, no hash needed -->
	<link rel="icon" href="/favicon.svg" type="image/svg+xml" />
	<!-- PNG fallback for older browsers; src kept in sync by regen_icons.sh -->
	<link rel="icon" type="image/png" href="/icon-512-bb5c0267.png" />
</svelte:head>

{@render children()}
