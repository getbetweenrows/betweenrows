import { defineConfig } from 'vitest/config'
import react from '@vitejs/plugin-react'

export default defineConfig({
  plugins: [react()],
  test: {
    environment: 'jsdom',
    // Pin jsdom's document URL to a closed TCP port. Axios in our client
    // uses baseURL: '/api/v1', which jsdom resolves against window.location.
    // Vitest's default http://localhost:3000/ means any unmocked test call
    // leaks to whatever happens to listen on :3000 (e.g. another vite dev
    // server on the same machine), which returns HTML for any path and
    // crashes components that try to .map()/.length the "response". Port 1
    // is guaranteed closed in practice — axios gets connection-refused in
    // ~1ms, the query enters error state, and defensive render paths kick in.
    environmentOptions: {
      jsdom: { url: 'http://localhost:1/' },
    },
    globals: true,
    css: false,
    setupFiles: ['./src/test/setup.ts'],
  },
})
