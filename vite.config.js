import { defineConfig } from "vite";

export default defineConfig({
  root: "web",
  build: {
    outDir: "../dist",
    emptyOutDir: true,
    // Three's core is loaded only after a table opens; its 546 KiB minified chunk is within this budget.
    chunkSizeWarningLimit: 600,
    rollupOptions: { output: { manualChunks: (id) => id.includes("/node_modules/three/") ? "three" : undefined } },
  },
  server: { proxy: { "/api": process.env.API_PROXY_TARGET || "http://127.0.0.1:8080" } },
});
