import { defineConfig } from "vite";
import { viteSingleFile } from "vite-plugin-singlefile";

const adapter = process.env.ADAPTER === "milkdown" ? "milkdown" : "tiptap";
export default defineConfig({
  plugins: [viteSingleFile()],
  build: {
    outDir: `dist-${adapter}`,
    target: "safari16",
    rollupOptions: { input: `${adapter}.html` },
  },
});
