import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
// noVNC uses top-level await, which needs an ES2022 build target.
export default defineConfig({ plugins: [react()], server: { port: 1428, strictPort: true }, clearScreen: false, build: { target: 'es2022' } });
