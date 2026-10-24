/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

// My Flow's encryption on the phone, matching haku-core's `flow.rs`: XChaCha20-Poly1305, sealed
// data is base64url(nonce || ciphertext || tag), and the additional data names the device and
// what the data is for. The page is served over plain HTTP on the local network, where
// WebCrypto's subtle API is unavailable, so the cipher comes from @noble/ciphers (audited, MIT);
// crypto.getRandomValues works on any page.

import { xchacha20poly1305 } from "@noble/ciphers/chacha.js";
import { randomBytes } from "@noble/ciphers/utils.js";

const encoder = new TextEncoder();
const decoder = new TextDecoder();

export function toBase64Url(bytes: Uint8Array): string {
  let s = "";
  for (let i = 0; i < bytes.length; i += 0x8000) s += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  return btoa(s).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

export function fromBase64Url(text: string): Uint8Array {
  const b64 = text.replace(/-/g, "+").replace(/_/g, "/");
  const bin = atob(b64 + "=".repeat((4 - (b64.length % 4)) % 4));
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
  return out;
}

/** Standard base64 (file contents inside messages). */
export function toBase64(bytes: Uint8Array): string {
  let s = "";
  for (let i = 0; i < bytes.length; i += 0x8000) s += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  return btoa(s);
}

export const aad = (device: string, purpose: string) => `hakucx-flow/1 ${device} ${purpose}`;

export function sealBytes(key: Uint8Array, additional: string, plaintext: Uint8Array): Uint8Array {
  const nonce = randomBytes(24);
  const sealed = xchacha20poly1305(key, nonce, encoder.encode(additional)).encrypt(plaintext);
  const out = new Uint8Array(24 + sealed.length);
  out.set(nonce);
  out.set(sealed, 24);
  return out;
}

export function openBytes(key: Uint8Array, additional: string, data: Uint8Array): Uint8Array | null {
  if (data.length < 24 + 16) return null;
  try {
    return xchacha20poly1305(key, data.subarray(0, 24), encoder.encode(additional)).decrypt(data.subarray(24));
  } catch {
    return null;
  }
}

export const seal = (key: Uint8Array, additional: string, value: unknown) => toBase64Url(sealBytes(key, additional, encoder.encode(JSON.stringify(value))));

export function open<T>(key: Uint8Array, additional: string, sealed: string): T | null {
  const plain = openBytes(key, additional, fromBase64Url(sealed));
  if (!plain) return null;
  try {
    return JSON.parse(decoder.decode(plain)) as T;
  } catch {
    return null;
  }
}

/** A request's freshness fields: a random id (accepted once) and the time. */
export const stamp = () => ({ id: toBase64Url(randomBytes(12)), t: Date.now() });
