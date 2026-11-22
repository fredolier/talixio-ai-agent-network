import path from 'node:path'
import tailwindcss from '@tailwindcss/vite'
import react from '@vitejs/plugin-react'
import { defineConfig } from 'vite'

// Le résultat (`dist/`) est embarqué dans l'exécutable Rust : noms de fichiers fixes, un seul
// script, une seule feuille de style, aucune ressource venue d'ailleurs.
export default defineConfig({
  plugins: [react(), tailwindcss()],
  resolve: { alias: { '@': path.resolve(import.meta.dirname, './src') } },
  build: {
    assetsInlineLimit: 0,
    cssCodeSplit: false,
    sourcemap: false,
    rollupOptions: {
      output: {
        entryFileNames: 'app.js',
        chunkFileNames: 'app-[name].js',
        assetFileNames: (asset) => {
          const name = asset.names?.[0] ?? ''
          if (name.endsWith('.css')) return 'app.css'
          if (name.startsWith('instrument-sans')) return 'sans.woff2'
          if (name.startsWith('jetbrains-mono')) return 'mono.woff2'
          return '[name][extname]'
        },
      },
    },
  },
  server: { proxy: { '/api': { target: 'https://127.0.0.1:7443', secure: false } } },
})
