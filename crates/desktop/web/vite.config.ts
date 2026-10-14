import { defineConfig } from "vite";
import { viteSingleFile } from "vite-plugin-singlefile";
// 单文件产物 dist/cm.html:提交进 Git,由 digger-desktop 用 include_str! 嵌入(ADR-013 §6)。
export default defineConfig({
  plugins: [viteSingleFile()],
  build: { outDir: "dist", target: "safari16", rollupOptions: { input: "cm.html" } },
});
