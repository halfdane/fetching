// Minimal service worker — makes the app installable as a PWA.
// A fetch handler is required for Chrome's installability heuristic.

self.addEventListener('install', () => self.skipWaiting());

self.addEventListener('activate', (e) => e.waitUntil(clients.claim()));

// Pass all requests through unchanged — no caching strategy yet.
self.addEventListener('fetch', (e) => {
  e.respondWith(fetch(e.request));
});
