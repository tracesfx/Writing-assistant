/**
 * Settings → Backups (SPEC §11.3): schedule, folder, "Back up now", and
 * restore. Restoring unpacks a backup as a new project folder next to the
 * current one and offers to open it; nothing is overwritten.
 */
import { open } from "@tauri-apps/plugin-dialog";
import { useEffect, useState } from "react";

import { editorHost, events, story, workbench, type BackupStatus } from "@margin/sdk";

const errorMessage = (e: unknown) => (e instanceof Error ? e.message : String(e));

function size(bytes: number): string {
  if (bytes < 1024 * 1024) return `${Math.max(1, Math.round(bytes / 1024))} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

export function BackupsPage() {
  const [status, setStatus] = useState<BackupStatus | null>(null);
  const [busy, setBusy] = useState(false);

  const load = () =>
    story.backups
      .status()
      .then(setStatus)
      .catch((e: unknown) => workbench.notify({ message: `Could not read backups: ${errorMessage(e)}` }));

  useEffect(() => {
    void load();
    return events.on("backup.finished", () => void load());
  }, []);

  if (!status) return <p className="muted">Loading…</p>;
  const cfg = status.config;
  const set = (patch: Partial<typeof cfg>) => void story.backups.setConfig({ ...cfg, ...patch }).then(setStatus);

  async function chooseFolder() {
    const dir = await open({ directory: true, title: "Folder for backups" });
    if (typeof dir === "string") set({ dir });
  }

  async function backupNow() {
    setBusy(true);
    try {
      await editorHost.flushAll();
      const b = await story.backups.now();
      workbench.notify({ message: `Backed up to ${b.name}` });
    } catch (e) {
      workbench.notify({ message: `Backup failed: ${errorMessage(e)}` });
    } finally {
      setBusy(false);
      void load();
    }
  }

  async function restore(path: string) {
    try {
      const root = await story.backups.restore(path);
      workbench.notify({
        message: `Restored as a copy: ${root}`,
        action: { label: "Open it", run: () => workbench.openProject(root) },
        timeoutMs: 0,
      });
    } catch (e) {
      workbench.notify({ message: `Could not restore: ${errorMessage(e)}` });
    }
  }

  async function restoreOther() {
    const file = await open({ title: "Choose a Margin backup", filters: [{ name: "Margin backup", extensions: ["zip"] }] });
    if (typeof file === "string") await restore(file);
  }

  return (
    <section>
      <h2>Backups</h2>
      <div className="settings-row">
        <div className="settings-label">
          <span>Automatic backups</span>
          <span className="settings-hint">A zip of the project whenever something changed, and when Margin closes.</span>
        </div>
        <div className="settings-control">
          <input type="checkbox" aria-label="Automatic backups" checked={cfg.enabled} onChange={(e) => set({ enabled: e.target.checked })} />
        </div>
      </div>
      <div className="settings-row">
        <div className="settings-label">
          <span>Every</span>
        </div>
        <div className="settings-control">
          <select aria-label="Backup interval" value={cfg.intervalMinutes} onChange={(e) => set({ intervalMinutes: Number(e.target.value) })}>
            {[15, 30, 60, 120, 240].map((m) => (
              <option key={m} value={m}>
                {m < 60 ? `${m} minutes` : `${m / 60} hour${m > 60 ? "s" : ""}`}
              </option>
            ))}
          </select>
        </div>
      </div>
      <div className="settings-row">
        <div className="settings-label">
          <span>Folder</span>
          <span className="settings-hint">A cloud-synced folder (e.g. OneDrive) keeps a copy off this computer.</span>
        </div>
        <div className="settings-control column">
          <code className="path">{status.folder}</code>
          <div>
            <button onClick={() => void chooseFolder()}>Choose folder…</button>
            {cfg.dir && <button onClick={() => set({ dir: null })}>Use default</button>}
          </div>
        </div>
      </div>
      <p className="settings-hint">Kept: the 10 most recent, one a day for two weeks, and one a week for eight weeks.</p>
      <div className="settings-actions">
        <button className="primary" disabled={busy} onClick={() => void backupNow()}>
          {busy ? "Backing up…" : "Back up now"}
        </button>
        <button onClick={() => void restoreOther()}>Restore a backup file…</button>
      </div>
      <h3>Backups of this project</h3>
      {status.backups.length === 0 && <p className="muted">None yet.</p>}
      <table className="backup-table">
        <tbody>
          {status.backups.map((b) => (
            <tr key={b.path}>
              <td>{b.createdAt}</td>
              <td className="muted">{size(b.sizeBytes ?? 0)}</td>
              <td>
                <button onClick={() => void restore(b.path)}>Restore as a copy</button>
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </section>
  );
}
