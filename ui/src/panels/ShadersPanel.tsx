/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

import { Radio, Toggle } from "../components/controls";
import { SHADERS } from "../lib/catalog";
import { patch } from "../lib/util";
import { useStore } from "../state/store";

export function ShadersPanel() {
  const features = useStore((s) => s.settings?.features);
  if (!features) return null;
  return (
    <div className="panel-scroll">
      <div className="setting-line">
        <div>
          <div>Shaders</div>
          <small className="muted">Shaders apply visual effects on web pages. This may impact performance.</small>
        </div>
        <Toggle on={features.shadersEnabled} onChange={(v) => patch({ features: { shadersEnabled: v } })} />
      </div>
      <div className="radio-list">
        <Radio checked={features.shader === "none"} onChange={() => patch({ features: { shader: "none" } })} label="None" />
        <h3 className="display section-label">GX</h3>
        {SHADERS.filter((s) => s.id !== "none").map((s) => (
          <Radio key={s.id} checked={features.shader === s.id} onChange={() => patch({ features: { shader: s.id, shadersEnabled: true } })} label={s.name} description={s.description} />
        ))}
      </div>
    </div>
  );
}
