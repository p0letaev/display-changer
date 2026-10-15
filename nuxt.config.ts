// Tauri serves static files, so Nuxt runs as a pure SPA (no SSR, no server).
export default defineNuxtConfig({
  compatibilityDate: '2026-10-01',
  ssr: false,
  modules: ['@pinia/nuxt', '@vueuse/nuxt'],
  css: ['~/assets/css/main.css'],
  app: {
    head: {
      title: 'Display Changer',
      htmlAttrs: { lang: 'ru' },
      meta: [{ name: 'color-scheme', content: 'dark light' }],
    },
  },
  devtools: { enabled: false },
  telemetry: false,
  typescript: { strict: true },
  ignore: ['src-tauri/**', 'target/**', 'crates/**'],
  devServer: { host: process.env.TAURI_DEV_HOST || 'localhost', port: 3000 },
  vite: {
    clearScreen: false,
    envPrefix: ['VITE_', 'TAURI_'],
    optimizeDeps: { include: ['@tauri-apps/api/core', '@tauri-apps/api/event'] },
    server: {
      strictPort: true,
      watch: { ignored: ['**/src-tauri/**', '**/target/**', '**/crates/**'] },
    },
  },
})
