/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

import { StrictMode, useState } from "react";
import { createRoot } from "react-dom/client";
import { create } from "zustand";

const useCounter = create<{ n: number; inc: () => void }>((set) => ({
  n: 0,
  inc: () => set((s) => ({ n: s.n + 1 })),
}));

function App() {
  const { n, inc } = useCounter();
  const [items, setItems] = useState(["alpha", "beta"]);
  return (
    <div style={{ padding: 40, color: "#fff", background: "#121217", height: "100vh", font: "16px system-ui" }}>
      <h1 style={{ color: "#7d46f2" }}>React {n}</h1>
      <button id="inc" onClick={() => { inc(); setItems((i) => [...i, `item ${i.length}`]); }}>increment</button>
      <ul>{items.map((i) => <li key={i}>{i}</li>)}</ul>
    </div>
  );
}

createRoot(document.getElementById("root")!).render(<StrictMode><App /></StrictMode>);
