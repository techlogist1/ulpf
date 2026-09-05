import { defineConfig } from 'vite'
import { svelte } from '@sveltejs/vite-plugin-svelte'

// Fixed output names: the Rust binary embeds dist/index.html, dist/app.js, dist/app.css
// with include_str! and serves them at /, /app.js, /app.css.
export default defineConfig({
  // List rows are divs reached by j/k/Enter at the screen level (keys.js), so the per-row
  // key-handler warning is noise here.
  plugins: [svelte({ onwarn: (w, handler) => { if (w.code !== 'a11y_click_events_have_key_events') handler(w) } })],
  base: '/',
  build: {
    cssCodeSplit: false,
    assetsInlineLimit: 1e9,
    modulePreload: false,
    rollupOptions: {
      output: {
        codeSplitting: false,
        entryFileNames: 'app.js',
        chunkFileNames: 'app.js',
        assetFileNames: (a) => (a.names ?? [a.name]).some((n) => n?.endsWith('.css')) ? 'app.css' : '[name][extname]',
      },
    },
  },
  server: { proxy: { '/api': 'http://127.0.0.1:7878' } },
})
