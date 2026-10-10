import { sveltekit } from '@sveltejs/kit/vite';
import { defineConfig } from 'vite';
import clayglNoEval from './vite/claygl-no-eval.js';

export default defineConfig({
  plugins: [clayglNoEval(), sveltekit()],
  // Pre-bundled deps skip plugin transforms: keep the 3D stack out so the dev server
  // gets the same eval-free claygl as the build.
  optimizeDeps: { exclude: ['echarts-gl', 'claygl'] },
  server: {
    host: true,
    port: 5173
  }
});
