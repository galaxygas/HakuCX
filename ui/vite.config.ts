/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

import react from "@vitejs/plugin-react";
import { defineConfig } from "vitest/config";

// The UI is served from haku://ui/ inside the browser, so every asset path is relative.
export default defineConfig({
  base: "./",
  plugins: [react()],
  build: {
    outDir: "dist",
    emptyOutDir: true,
    target: "es2022",
    // Servo loads everything from the binary; no preload polyfill or inlining tricks needed.
    modulePreload: false,
    assetsInlineLimit: 0,
    sourcemap: false,
  },
  test: {
    environment: "node",
    setupFiles: ["src/test/setup.ts"],
  },
});
