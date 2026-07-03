import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

// Browser polyfills for the Solana/Anchor stack (Buffer, global, process.env).
export default defineConfig({
  plugins: [react()],
  define: {
    global: 'globalThis',
    'process.env': {},
  },
  resolve: {
    alias: {
      buffer: 'buffer/',
    },
  },
})
