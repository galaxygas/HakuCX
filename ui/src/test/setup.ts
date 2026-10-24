/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

// Tests run in Node, without a DOM. The IPC bridge installs itself on `window` and listens for
// errors, and commands are logged with console.debug, so give it a minimal window and keep
// test output quiet.
const g = globalThis as unknown as { window?: unknown; addEventListener?: unknown };
g.addEventListener ??= () => {};
g.window ??= globalThis;
console.debug = () => {};
