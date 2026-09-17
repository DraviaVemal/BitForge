import { useEffect, useState } from "react";
import type { CacheInfo } from "../lib/api";
import { api } from "../lib/api";
import { confirmDialog } from "../lib/confirm";
import { formatBytes } from "../lib/format";

export default function Cache() {
  const [info, setInfo] = useState<CacheInfo | null>(null);
  const [message, setMessage] = useState("");
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    let active = true;
    const load = async () => {
      try {
        const data = await api.cache();
        if (active) setInfo(data);
      } catch (error) {
        if (active) setMessage(String((error as Error).message ?? error));
      }
    };
    void load();
    const timer = window.setInterval(() => void load(), 2500);
    return () => {
      active = false;
      window.clearInterval(timer);
    };
  }, []);

  const clear = async () => {
    const confirmed = await confirmDialog(
      "This permanently deletes all downloads, the shared-state cache, build output (TMPDIR) and build history under build/.\n\n" +
        "Every source will be re-downloaded and every recipe rebuilt from scratch on the next build, which can take a very long time.\n\n" +
        "Continue and clear the cache and downloads?",
      {
        title: "Clear cache & downloads",
        confirmText: "Clear everything",
        cancelText: "Keep",
        danger: true,
      },
    );
    if (!confirmed) return;
    setBusy(true);
    setMessage("Clearing cache and downloads…");
    try {
      const result = await api.clearCache();
      setInfo(await api.cache());
      setMessage(result.cleared.length ? `Cleared: ${result.cleared.join(", ")}.` : "Nothing to clear.");
    } catch (error) {
      setMessage(String((error as Error).message ?? error));
    } finally {
      setBusy(false);
    }
  };

  const scanning = info?.scanning ?? !info;

  return (
    <section className="page">
      <div className="page-head">
        <h1>Cache &amp; Downloads</h1>
        <button className="danger" onClick={clear} disabled={busy}>
          Clear cache &amp; downloads
        </button>
      </div>

      {scanning && (
        <p className="scanning-note">
          <span className="task-spin" /> Updating recent values…
        </p>
      )}

      {info && (
        <div className="cards">
          <div className="card">
            <span>Total on disk</span>
            <strong>{formatBytes(info.total_bytes)}</strong>
          </div>
          <div className="card">
            <span>Downloads</span>
            <strong>{formatBytes(info.downloads_bytes)}</strong>
          </div>
          <div className="card">
            <span>Cache (sstate + tmp)</span>
            <strong>{formatBytes(info.cache_bytes)}</strong>
          </div>
          <div className="card">
            <span>Stores present</span>
            <strong>{info.entries.filter((entry) => entry.exists).length}/{info.entries.length}</strong>
          </div>
        </div>
      )}

      {message && <p className="muted">{message}</p>}

      {info && (
        <table className="table">
          <thead>
            <tr>
              <th>Store</th>
              <th>Path</th>
              <th>Size</th>
              <th>Files</th>
              <th>Status</th>
            </tr>
          </thead>
          <tbody>
            {info.entries.map((entry) => (
              <tr key={entry.key}>
                <td>{entry.label}</td>
                <td>
                  <code>{entry.path}</code>
                </td>
                <td>{formatBytes(entry.bytes)}</td>
                <td>{entry.files.toLocaleString()}</td>
                <td>
                  <span className={`badge ${entry.exists ? "ok" : "warn"}`}>
                    {entry.exists ? "present" : "empty"}
                  </span>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </section>
  );
}
