import { svelte } from '@sveltejs/vite-plugin-svelte'
import { defineConfig } from 'vite'

// In development the Rust server runs on :8610 and Vite proxies the API to it.
export default defineConfig({
  plugins: [svelte()],
  // The graph view chunk is ~1.6MB because of the ELK layout engine. It is
  // lazy-loaded (see App.svelte), so the size doesn't affect first load.
  build: { chunkSizeWarningLimit: 2000 },
  server: {
    port: 5173,
    proxy: { '/api': 'http://127.0.0.1:8610' },
  },
})
