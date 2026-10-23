/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

// Confirmations for extensions: adding one (with what it can do), and an extension asking
// for more permissions (`permissions.request`).

import { Button } from "../components/controls";
import { Icon } from "../components/Icon";
import { send } from "../ipc/bridge";
import { useStore } from "../state/store";

function Warnings({ warnings }: { warnings: string[] }) {
  if (!warnings.length) return <p className="muted">It needs no special permissions.</p>;
  return (
    <>
      <p>It can:</p>
      <ul className="ext-list">
        {warnings.map((w) => (
          <li key={w}>{w}</li>
        ))}
      </ul>
    </>
  );
}

export function ExtPrompts() {
  const install = useStore((s) => s.extInstall);
  const permission = useStore((s) => s.extPermissions[0]);
  if (install) {
    const answer = (accept: boolean) => {
      send({ type: "ext-install-answer", request: install.request, accept });
      useStore.setState({ extInstall: null });
    };
    return (
      <div className="modal-backdrop">
        <div className="modal ext-prompt" role="dialog" style={{ width: 440 }}>
          <div className="modal-head">
            <Icon name="puzzle" size={20} />
            <h2 className="display">
              {install.update ? "Update" : "Add"} "{install.name}"?
            </h2>
          </div>
          <div className="modal-body">
            <p className="muted">Version {install.version}</p>
            <Warnings warnings={install.warnings} />
          </div>
          <div className="modal-foot">
            <span className="grow" />
            <Button onClick={() => answer(false)}>Cancel</Button>
            <Button kind="accent" onClick={() => answer(true)}>
              {install.update ? "Update extension" : "Add extension"}
            </Button>
          </div>
        </div>
      </div>
    );
  }
  if (permission) {
    const answer = (grant: boolean) => {
      send({ type: "ext-permission-answer", request: permission.request, grant });
      useStore.setState((s) => ({ extPermissions: s.extPermissions.slice(1) }));
    };
    return (
      <div className="modal-backdrop">
        <div className="modal ext-prompt" role="dialog" style={{ width: 440 }}>
          <div className="modal-head">
            <Icon name="shield" size={20} />
            <h2 className="display">"{permission.name}" asks for more access</h2>
          </div>
          <div className="modal-body">
            <Warnings warnings={permission.warnings} />
          </div>
          <div className="modal-foot">
            <span className="grow" />
            <Button onClick={() => answer(false)}>Deny</Button>
            <Button kind="accent" onClick={() => answer(true)}>
              Allow
            </Button>
          </div>
        </div>
      </div>
    );
  }
  return null;
}
