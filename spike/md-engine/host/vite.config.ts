import { defineConfig } from "vite";
import { viteSingleFile } from "vite-plugin-singlefile";
const engine = process.env.ENGINE === "pm" ? "pm" : "cm";
export default defineConfig({
  plugins: [viteSingleFile()],
  build: { outDir: `dist-${engine}`, target: "safari16", rollupOptions: { input: `${engine}.html` } },
});
