import { defineConfig } from 'vite'
import { svelte } from '@sveltejs/vite-plugin-svelte'

// Fixed output names: the Rust binary embeds dist/index.html, dist/app.js, dist/app.css
// with include_str! and serves them at /, /app.js, /app.css.
export default defineConfig({
  plugins: [svelte()],
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
