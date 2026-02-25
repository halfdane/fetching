<script lang="ts">
	import QueueView from '../lib/QueueView.svelte';
	import DevDrawer from '../lib/DevDrawer.svelte';
</script>

<main class="min-h-screen flex flex-col items-center bg-gradient-to-br from-black via-gray-900 to-gray-800 text-white">
	<h1 class="text-4xl font-bold mt-12 mb-4">Fetching PWA Prototype</h1>
	<p class="text-lg mb-8">This page is powered by SvelteKit + Tailwind CSS.<br>It replaces the old static HTML and is ready for your UX vision.</p>
	<QueueView />
</main>

<DevDrawer />
