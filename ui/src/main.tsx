/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

// The browser UI. Rendered by Servo in a transparent webview layered over the pages; it talks
// to the Rust core only through ipc/bridge.ts.

import "@fontsource/outfit/latin-400.css";
import "@fontsource/outfit/latin-500.css";
import "@fontsource/outfit/latin-600.css";
import "@fontsource/oxanium/latin-500.css";
import "@fontsource/oxanium/latin-600.css";
import "@fontsource/oxanium/latin-700.css";
import "./styles/base.css";
import "./styles/shell.css";
import "./styles/overlays.css";
import "./styles/panels.css";
import "./styles/pages.css";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import { startRewardTracking } from "./lib/rewards";

startRewardTracking();

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
