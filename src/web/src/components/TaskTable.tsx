import { useEffect, useMemo, useState } from "react";
import type { BuildSnapshot, BuildTaskRow } from "../lib/api";
import { api } from "../lib/api";
import { copyToClipboard, formatDuration, shortSha } from "../lib/format";

const ROW_LIMIT = 1000;

const STATUS_LABEL: Record<string, string> = {
  running: "running",
  built: "built",
  cached: "from sstate cache",
  failed: "failed",
  queued: "queued",
};

function statusCell(status: string) {
  switch (status) {
    case "running":
      return <span className="task-spin" title="running" />;
    case "built":
      return (
        <span className="tick double" title="built">
          ✓✓
        </span>
      );
    case "cached":
      return (
        <span className="tick single" title="from sstate cache">
          ✓
        </span>
      );
    case "failed":
      return (
        <span className="tick fail" title="failed">
          ✗
        </span>
      );
    default:
      return (
        <span className="tick empty" title="queued">
          ·
        </span>
      );
  }
}

function elapsedSeconds(row: BuildTaskRow, nowSeconds: number): number | null {
  if (row.started_at == null) return null;
  const end = row.finished_at ?? nowSeconds;
  return Math.max(0, end - row.started_at);
}

function formatClock(seconds?: number | null): string {
  if (!seconds) return "—";
  return new Date(seconds * 1000).toLocaleTimeString();
}

export default function TaskTable({ active }: { active: BuildSnapshot | null }) {
  const [rows, setRows] = useState<BuildTaskRow[]>([]);
  const [now, setNow] = useState(() => Math.floor(Date.now() / 1000));
  const [selected, setSelected] = useState<BuildTaskRow | null>(null);

  const running = active?.status === "running";

  useEffect(() => {
    const load = async () => {
      try {
        setRows(await api.buildTasks());
      } catch {}
    };
    void load();
    const timer = running ? window.setInterval(() => void load(), 2000) : undefined;
    return () => {
      if (timer) window.clearInterval(timer);
    };
  }, [running]);

  useEffect(() => {
    if (!running) return;
    const clock = window.setInterval(() => setNow(Math.floor(Date.now() / 1000)), 1000);
    return () => window.clearInterval(clock);
  }, [running]);

  const detail = useMemo(() => {
    if (!selected) return null;
    return (
      rows.find((row) => row.recipe === selected.recipe && row.task === selected.task) ?? selected
    );
  }, [rows, selected]);

  if (rows.length === 0) return null;

  const shown = rows.slice(0, ROW_LIMIT);

  return (
    <div className="task-table-wrap">
      <div className="task-legend muted small">
        <span>
          <span className="task-spin" /> running
        </span>
        <span>
          <span className="tick double">✓✓</span> built
        </span>
        <span>
          <span className="tick single">✓</span> from cache
        </span>
        <span>
          <span className="tick fail">✗</span> failed
        </span>
        <span>
          <span className="tick empty">·</span> queued
        </span>
        <span className="muted">· click a task for details</span>
      </div>
      <table className="table task-table-full">
        <thead>
          <tr>
            <th className="task-status-col">Status</th>
            <th>Recipe</th>
            <th>Task</th>
            <th className="task-elapsed-col num">Elapsed</th>
          </tr>
        </thead>
        <tbody>
          {shown.map((row) => {
            const elapsed = elapsedSeconds(row, now);
            return (
              <tr
                key={`${row.recipe}:${row.task}`}
                className={`task-row ${row.status} task-clickable`}
                onClick={() => setSelected(row)}
              >
                <td className="task-status-col">{statusCell(row.status)}</td>
                <td>
                  <code>{row.recipe}</code>
                </td>
                <td>{row.task}</td>
                <td className="task-elapsed-col num">
                  {elapsed == null ? "—" : formatDuration(elapsed)}
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
      {rows.length > ROW_LIMIT && (
        <p className="muted small">
          Showing {ROW_LIMIT.toLocaleString()} of {rows.length.toLocaleString()} tasks.
        </p>
      )}

      {detail && (
        <div className="modal-overlay" onClick={() => setSelected(null)}>
          <div className="modal" onClick={(event) => event.stopPropagation()}>
            <div className="modal-head">
              <strong>
                <code>{detail.recipe}</code> · {detail.task}
              </strong>
              <button className="ghost-btn" onClick={() => setSelected(null)}>
                Close
              </button>
            </div>
            <div className="cards">
              <div className="card">
                <span>Status</span>
                <strong>
                  <span className="task-detail-status">
                    {statusCell(detail.status)} {STATUS_LABEL[detail.status] ?? detail.status}
                  </span>
                </strong>
              </div>
              <div className="card">
                <span>Elapsed</span>
                <strong>
                  {(() => {
                    const elapsed = elapsedSeconds(detail, now);
                    return elapsed == null ? "—" : formatDuration(elapsed);
                  })()}
                </strong>
              </div>
              <div className="card">
                <span>Started</span>
                <strong className="mono-small">{formatClock(detail.started_at)}</strong>
              </div>
              <div className="card">
                <span>Finished</span>
                <strong className="mono-small">{formatClock(detail.finished_at)}</strong>
              </div>
            </div>
            {detail.signature && (
              <p className="muted small path-row">
                Signature: <code>{shortSha(detail.signature, 16)}</code>
                <button className="link" onClick={() => copyToClipboard(detail.signature ?? "")}>
                  copy
                </button>
              </p>
            )}
          </div>
        </div>
      )}
    </div>
  );
}
