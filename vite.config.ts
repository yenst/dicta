import { defineConfig } from "vite";

export default defineConfig({
  // Tauri serves production assets from an embedded custom-protocol URL.
  // Relative URLs keep JS, CSS, fonts, and images beside index.html in every bundle.
  base: "./",
});
