/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

import { describe, expect, it } from "vitest";
import { aad, fromBase64Url, open, openBytes, seal, sealBytes, stamp, toBase64Url } from "./crypto";

describe("My Flow crypto (phone page)", () => {
  it("opens what HakuCX (haku-core flow.rs) seals", () => {
    // Sealed by `flow::seal(&[2; 32], "hakucx-flow/1 dev inbox-reply", ...)` in Rust.
    const sealed = "ApyQOFGyMQiJZt1YC-wlMHCnAYITUrCeGZ1uEVfI2DR0N2AnmPISKogQIP_Tr0Ns2YI4epvIFNnBqUkl9Q";
    expect(open(new Uint8Array(32).fill(2), "hakucx-flow/1 dev inbox-reply", sealed)).toEqual({ hello: "from rust" });
  });

  it("round-trips and authenticates the purpose", () => {
    const key = new Uint8Array(32).fill(9);
    const s = seal(key, aad("d1", "send"), { a: 1 });
    expect(open(key, aad("d1", "send"), s)).toEqual({ a: 1 });
    expect(open(key, aad("d1", "inbox"), s)).toBeNull();
    expect(open(new Uint8Array(32), aad("d1", "send"), s)).toBeNull();
    const bytes = new Uint8Array([1, 2, 3]);
    expect(openBytes(key, "x", sealBytes(key, "x", bytes))).toEqual(bytes);
  });

  it("encodes base64url and stamps requests", () => {
    const b = new Uint8Array([251, 255, 0, 1]);
    expect(toBase64Url(b)).toBe("-_8AAQ");
    expect(fromBase64Url("-_8AAQ")).toEqual(b);
    const s1 = stamp();
    expect(s1.id).not.toBe(stamp().id);
    expect(Math.abs(s1.t - Date.now())).toBeLessThan(1000);
  });
});
