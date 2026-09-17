import { useEffect, useMemo, useState } from "react";
import type { Builds as BuildsData, BuildRecord } from "../lib/api";
import { api } from "../lib/api";
import { copyToClipboard, formatDuration } from "../lib/format";
import DataTable, { type Column } from "../components/DataTable";

function elapsedLabel(record: BuildRecord): string {
  if (record.elapsed_secs != null) return formatDuration(record.elapsed_secs);
  if (record.status === "running") return "…";
  return "—";
}

function formatTimestamp(seconds?: number | null): string {
  if (!seconds) return "—";
  return new Date(seconds * 1000).toLocaleString();
}

function downloadLog(buildId: number, contents: string) {
  const blob = new Blob([contents], { type: "text/plain" });
  const url = URL.createObjectURL(blob);
  const anchor = document.createElement("a");
  anchor.href = url;
  anchor.download = `build-${buildId}.log`;
  anchor.click();
  URL.revokeObjectURL(url);
}

export default function Builds() {
  const [data, setData] = useState<BuildsData | null>(null);
  const [selected, setSelected] = useState<BuildRecord | null>(null);
  const [logText, setLogText] = useState("");
  const [logError, setLogError] = useState("");
  const [logBusy, setLogBusy] = useState(false);

  useEffect(() => {
    const load = async () => {
      try {
        setData(await api.builds());
      } catch {}
    };
    void load();
    const timer = window.setInterval(() => void load(), 1500);
    return () => window.clearInterval(timer);
  }, []);

  const openDetails = async (record: BuildRecord) => {
    setSelected(record);
    setLogText("Loading log…");
    setLogError("");
    setLogBusy(true);
    try {
      setLogText(await api.buildLog(record.id));
    } catch (error) {
      setLogError(String((error as Error).message ?? error));
      setLogText("");
    } finally {
      setLogBusy(false);
    }
  };

  const columns: Column<BuildRecord>[] = useMemo(
    () => [
      { key: "id", header: "#", type: "number", width: "72px", value: (row) => row.id },
      {
        key: "target",
        header: "Target",
        type: "text",
        render: (row) => <code>{row.target}</code>,
        value: (row) => row.target,
      },
      {
        key: "status",
        header: "Status",
        type: "text",
        render: (row) => <span className={`badge status--${row.status}`}>{row.status}</span>,
        value: (row) => row.status,
      },
      {
        key: "started_at",
        header: "Started",
        type: "number",
        value: (row) => row.started_at,
        render: (row) => <span className="muted small">{formatTimestamp(row.started_at)}</span>,
      },
      {
        key: "elapsed",
        header: "Elapsed",
        type: "number",
        align: "right",
        value: (row) => row.elapsed_secs ?? 0,
        render: (row) => elapsedLabel(row),
      },
      {
        key: "actions",
        header: "Log",
        align: "right",
        render: (row) => (
          <button className="ghost-btn" onClick={() => openDetails(row)}>
            Details &amp; log
          </button>
        ),
      },
    ],
    [],
  );

  if (!data) {
    return (
      <section className="page">
        <h1>Build History</h1>
        <p className="muted">Loading…</p>
      </section>
    );
  }

  return (
    <section className="page">
      <div className="page-head">
        <div className="head-title">
          <h1>Build History</h1>
        </div>
        <span className="head-stats">
          <span className="head-stat">
            Builds<em>{data.history.length}</em>
          </span>
        </span>
      </div>

      {data.history.length === 0 ? (
        <p className="muted">No builds yet.</p>
      ) : (
        <DataTable
          columns={columns}
          rows={data.history}
          rowKey={(row) => row.id}
          initialSort={{ key: "id", direction: "desc" }}
          empty="No builds match the filters."
        />
      )}

      {selected && (
        <div className="modal-overlay" onClick={() => setSelected(null)}>
          <div className="modal task-modal" onClick={(event) => event.stopPropagation()}>
            <div className="modal-head">
              <strong>
                Build #{selected.id} · <code>{selected.target}</code>
              </strong>
              <div className="modal-actions">
                <button
                  className="ghost-btn"
                  disabled={logBusy || !logText}
                  onClick={() => downloadLog(selected.id, logText)}
                >
                  Download log
                </button>
                <button className="ghost-btn" onClick={() => setSelected(null)}>
                  Close
                </button>
              </div>
            </div>

            <div className="cards">
              <div className="card">
                <span>Status</span>
                <strong className={`status--${selected.status}`}>{selected.status}</strong>
              </div>
              <div className="card">
                <span>Elapsed</span>
                <strong>{elapsedLabel(selected)}</strong>
              </div>
              <div className="card">
                <span>Started</span>
                <strong className="mono-small">{formatTimestamp(selected.started_at)}</strong>
              </div>
              <div className="card">
                <span>Finished</span>
                <strong className="mono-small">{formatTimestamp(selected.finished_at)}</strong>
              </div>
            </div>

            {selected.report_dir && (
              <p className="muted small path-row">
                Report: <code>{selected.report_dir}</code>
                <button className="link" onClick={() => copyToClipboard(selected.report_dir ?? "")}>
                  copy
                </button>
              </p>
            )}
            {selected.log_path && (
              <p className="muted small path-row">
                Log file: <code>{selected.log_path}</code>
                <button className="link" onClick={() => copyToClipboard(selected.log_path ?? "")}>
                  copy
                </button>
              </p>
            )}

            {logError && <p className="error">{logError}</p>}
            <pre className="log task-log">{logText}</pre>
          </div>
        </div>
      )}
    </section>
  );
}
