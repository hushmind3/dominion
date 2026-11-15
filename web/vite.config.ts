import { defineConfig, searchForWorkspaceRoot } from 'vite'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'
const apiTarget=process.env.DOMINION_API_URL||'http://127.0.0.1:4317'
const webRoot=dirname(fileURLToPath(import.meta.url))
export default defineConfig({
  plugins: [react(), tailwindcss()],
  resolve: { alias: { '@dominion/sdk': resolve(webRoot,'../sdk/typescript/src/index.ts') } },
  server: {
    // Shared SDK source lives beside web/: allow only its source directory,
    // not an arbitrary parent filesystem tree, in the development server.
    fs: { allow: [searchForWorkspaceRoot(webRoot), resolve(webRoot,'../sdk/typescript/src')] },
    host: '127.0.0.1', port: 5173, strictPort: true,
    proxy: { '/api': {
      target: apiTarget, changeOrigin: true,
      configure(proxy) {
        proxy.on('proxyReq', (request, incoming) => {
          // Only this loopback Vite origin may stand in for the Rust origin.
          if (['http://127.0.0.1:5173', 'http://localhost:5173'].includes(incoming.headers.origin || '')) {
            request.setHeader('origin', new URL(apiTarget).origin)
          }
        })
      },
    } },
  },
  build: { outDir: 'dist' }
})
