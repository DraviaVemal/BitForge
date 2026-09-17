import { useEffect, useRef, useState } from "react";
import type { Connection } from "../lib/connection";
import type { TaskDetail as TaskDetailModel } from "../lib/api";
import { api } from "../lib/api";

function elapsed(start: number, end: number | null | undefined): string {
  const stop = end ?? Math.floor(Date.now() / 1000);
  const seconds = Math.max(0, stop - start);
  if (seconds < 60) return `${seconds}s`;
  return `${Math.floor(seconds / 60)}m ${seconds % 60}s`;
}

export default function TaskDetail({
  taskId,
  conn,
  onClose,
}: {
  taskId: number;
  conn: Connection;
  onClose: () => void;
}) {
  const [task, setTask] = useState<TaskDetailModel | null>(null);
  const [log, setLog] = useState<string[]>([]);
  const [busy, setBusy] = useState(false);
  const logRef = useRef<HTMLPreElement>(null);

  useEffect(() => {
    let active = true;
    const hydrateInit = async () => {
      try {
        const detail = await api.task(taskId);
        if (!active) return;
        setTask(detail);
        setLog(detail.log);
      } catch {}
    };
    void hydrateInit();
    const off = conn.onMessage((message) => {
      if (message.kind === "task-log") {
        const payload = message.payload as { id: number; line: string };
        if (payload.id === taskId) setLog((prev) => [...prev.slice(-800), payload.line]);
      }
      if (message.kind === "tasks") {
        const entries = message.payload as TaskDetailModel[];
        const match = entries.find((entry) => entry.id === taskId);
        if (match) setTask((prev) => (prev ? { ...prev, ...match } : prev));
      }
    });
    return () => {
      active = false;
      off();
    };
  }, [taskId, conn]);

  useEffect(() => {
    logRef.current?.scrollTo({ top: logRef.current.scrollHeight });
  }, [log]);

  const cancel = async () => {
    setBusy(true);
    try {
      await api.cancelTask(taskId);
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="modal-overlay" onClick={onClose}>
      <div className="modal task-modal" onClick={(event) => event.stopPropagation()}>
        <div className="modal-head">
          <div>
            <h3>{task?.label ?? `Task ${taskId}`}</h3>
            {task && (
              <span className="muted small">
                {task.kind} · {task.status} · {elapsed(task.started_at, task.finished_at)}
              </span>
            )}
          </div>
          <div className="modal-actions">
            {task?.status === "running" && task.cancellable && (
              <button className="ghost-btn danger" onClick={cancel} disabled={busy}>
                Cancel
              </button>
            )}
            <button className="ghost-btn" onClick={onClose}>
              Close
            </button>
          </div>
        </div>
        <pre ref={logRef} className="log task-log">
          {log.length ? log.join("\n") : "Waiting for output…"}
        </pre>
      </div>
    </div>
  );
}
