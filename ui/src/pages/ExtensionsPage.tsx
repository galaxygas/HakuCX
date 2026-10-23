/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

// Replaced by the full extensions manager when the WebExtensions runtime lands (milestone M6).
export function ExtensionsPage() {
  return (
    <div className="hub-main card">
      <h2 className="display">Extensions</h2>
      <p className="muted">The extensions runtime (Chrome and Firefox WebExtensions) is being built in milestone M6. Installing extensions is not available in this build.</p>
    </div>
  );
}
