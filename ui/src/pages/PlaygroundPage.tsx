/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

import { useState } from "react";
import { Button, Toggle } from "../components/controls";
import { Icon } from "../components/Icon";
import { send } from "../ipc/bridge";
import { EARLY_BIRD } from "../lib/catalog";
import { clsx } from "../lib/util";
import { useStore } from "../state/store";

export function PlaygroundPage() {
  const eb = useStore((s) => s.settings?.earlyBird);
  const [index, setIndex] = useState(0);
  if (!eb) return null;
  const feature = EARLY_BIRD[index]!;
  const on = (id: string) => !!eb.features[id];
  const setFeature = (id: string, value: boolean) => {
    let loadout = eb.loadout.filter((x) => x !== id);
    if (value) loadout = [...loadout, id].slice(-3);
    send({ type: "patch-settings", patch: { earlyBird: { enabled: true, features: { ...eb.features, [id]: value }, loadout } } });
  };

  if (!eb.playgroundIntroSeen) {
    return (
      <div className="playground-intro card">
        <h2 className="display">Welcome to the Playground</h2>
        <p>
          The Playground is where HakuCX tries new ideas before they become regular settings. Experiments can change or disappear between versions. Turn on up
          to three at a time in your loadout.
        </p>
        <Button kind="accent" onClick={() => send({ type: "patch-settings", patch: { earlyBird: { playgroundIntroSeen: true } } })}>
          Let's get started
        </Button>
      </div>
    );
  }

  return (
    <div className="playground">
      <div className="carousel">
        <button className="icon-btn" onClick={() => setIndex((i) => (i - 1 + EARLY_BIRD.length) % EARLY_BIRD.length)} title="Previous">
          <Icon name="chevron-left" size={22} />
        </button>
        <div className="carousel-card notch">
          <small className="display dim">
            Experiment {index + 1} / {EARLY_BIRD.length}
          </small>
          <h2 className="display">{feature.name}</h2>
          <p>{feature.description}</p>
          <div className="row">
            <Toggle on={on(feature.id)} onChange={(v) => setFeature(feature.id, v)} label={feature.name} />
            <span>{on(feature.id) ? "On" : "Off"}</span>
          </div>
        </div>
        <button className="icon-btn" onClick={() => setIndex((i) => (i + 1) % EARLY_BIRD.length)} title="Next">
          <Icon name="chevron-right" size={22} />
        </button>
      </div>
      <h3 className="display section-label">Loadout</h3>
      <div className="loadout">
        {[0, 1, 2].map((slot) => {
          const id = eb.loadout[slot];
          const f = EARLY_BIRD.find((x) => x.id === id);
          return (
            <div key={slot} className={clsx("loadout-slot notch", f && "filled")}>
              {f ? (
                <>
                  <span className="display">{f.name}</span>
                  <button className="icon-btn" title="Remove" onClick={() => setFeature(f.id, false)}>
                    <Icon name="x" size={13} />
                  </button>
                </>
              ) : (
                <span className="dim">Empty slot</span>
              )}
            </div>
          );
        })}
        <button className="loadout-slot notch early-bird" onClick={() => send({ type: "navigate", tab: null, input: "haku://settings/early-bird", disposition: "current-tab" })}>
          <span className="display">Early Bird</span>
          <Icon name="external" size={13} />
        </button>
      </div>
    </div>
  );
}
