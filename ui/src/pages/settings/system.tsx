/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

// Default browser, updates and crash reports.

import { Button, Toggle } from "../../components/controls";
import { Icon } from "../../components/Icon";
import { send } from "../../ipc/bridge";
import { patch } from "../../lib/util";
import { useStore } from "../../state/store";

export function DefaultBrowserPanel() {
  const info = useStore((s) => s.system);
  if (!info) return null;
  if (!info.windows) return <p className="muted">Setting the default browser is only available on Windows for now.</p>;
  return (
    <div className="setting-line">
      <div className="setting-line-text">
        <span>{info.isDefault ? "HakuCX is your default browser" : "HakuCX is not your default browser"}</span>
        <small className="muted">
          {info.isDefault
            ? "Links from other apps open here."
            : "HakuCX registers itself with Windows and opens Windows Settings, where you pick HakuCX for web browsing (Windows only lets you make this choice yourself)."}
        </small>
      </div>
      <span className="row">
        {info.registered && !info.isDefault && (
          <Button kind="ghost" title="Remove HakuCX from Windows' list of browsers" onClick={() => send({ type: "unregister-browser" })}>
            Unregister
          </Button>
        )}
        {!info.isDefault && (
          <Button kind="accent" onClick={() => send({ type: "make-default-browser" })}>
            Make default
          </Button>
        )}
        {info.isDefault && <Icon name="ok" size={18} />}
      </span>
    </div>
  );
}

const when = (ms: number | null) => (ms ? new Date(ms).toLocaleString() : "never");

export function UpdatePanel() {
  const status = useStore((s) => s.update);
  const info = useStore((s) => s.system);
  const auto = useStore((s) => s.settings?.updates.checkAutomatically ?? true);
  if (!status) return null;
  const openPage = () => status.page && send({ type: "new-tab", url: status.page, background: false, relativeTo: null });
  let line: string;
  switch (status.state) {
    case "unavailable":
      line = "This build was made without the update key, so it cannot update itself. New versions are published as GitHub releases.";
      break;
    case "checking":
      line = "Checking for updates...";
      break;
    case "up-to-date":
      line = `HakuCX is up to date (checked ${when(status.lastCheck)}).${status.message ? ` ${status.message}` : ""}`;
      break;
    case "available":
      line = `HakuCX ${status.version} is available. ${status.message}`;
      break;
    case "downloading":
      line = `Downloading HakuCX ${status.version}... ${Math.round(status.progress * 100)}%`;
      break;
    case "ready":
      line = `HakuCX ${status.version} is downloaded and verified. Restart to install it.`;
      break;
    case "error":
      line = status.message || "The update check failed.";
      break;
    default:
      line = status.lastCheck ? `Last checked ${when(status.lastCheck)}.` : "Not checked yet.";
  }
  return (
    <div className="update-panel">
      <div className="setting-line">
        <div className="setting-line-text">
          <span>
            HakuCX {status.current}
            {info && <span className="muted"> · {info.installed ? "installed" : info.portable ? "portable" : "not installed by the HakuCX installer"}</span>}
          </span>
          <small className={status.state === "error" ? "warn-text" : "muted"}>{line}</small>
          {status.state === "downloading" && (
            <div className="progress-track">
              <div className="progress-fill" style={{ width: `${Math.round(status.progress * 100)}%` }} />
            </div>
          )}
        </div>
        <span className="row">
          {status.page && (
            <Button kind="ghost" icon="external" onClick={openPage}>
              Release notes
            </Button>
          )}
          {status.state === "ready" ? (
            <Button kind="accent" icon="refresh" onClick={() => send({ type: "install-update" })}>
              Restart to update
            </Button>
          ) : (
            status.state !== "unavailable" && (
              <Button icon="refresh" disabled={status.state === "checking" || status.state === "downloading"} onClick={() => send({ type: "check-for-updates" })}>
                Check for updates
              </Button>
            )
          )}
        </span>
      </div>
      {status.state !== "unavailable" && (
        <div className="setting-line">
          <div className="setting-line-text">
            <span>Update automatically</span>
            <small className="muted">
              Checks GitHub once a day, downloads new versions and verifies their signature. HakuCX installs an update only when you restart it. Portable copies are told
              about new versions but update by hand.
            </small>
          </div>
          <Toggle on={auto} onChange={(v) => patch({ updates: { checkAutomatically: v } })} label="Update automatically" />
        </div>
      )}
    </div>
  );
}

export function CrashReportsRow() {
  const info = useStore((s) => s.system);
  return (
    <div className="setting-line">
      <div className="setting-line-text">
        <span>Crash reports</span>
        <small className="muted">
          If HakuCX crashes, a report is saved on this computer only; HakuCX never sends it anywhere.{" "}
          {info ? (info.crashReports === 0 ? "There are none." : `There ${info.crashReports === 1 ? "is 1 report" : `are ${info.crashReports} reports`}.`) : ""}
        </small>
      </div>
      <Button icon="folder-open" onClick={() => send({ type: "open-crash-reports" })}>
        Open folder
      </Button>
    </div>
  );
}
