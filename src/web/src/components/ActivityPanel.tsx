import { useEffect, useState, type MouseEvent } from "react";
import type { Connection, ConnectionMode } from "../lib/connection";
import type { TaskEntry } from "../lib/api";
import { api } from "../lib/api";
import TaskDetail from "./TaskDetail";

function elapsed(task: TaskEntry, nowSec: number): string {
  const end = task.finished_at ?? nowSec;
  const seconds = Math.max(0, end - task.started_at);
  if (seconds < 60) return `${seconds}s`;
  const minutes = Math.floor(seconds / 60);
  return `${minutes}m ${seconds % 60}s`;
}

const STATUS_ICON: Record<string, string> = {
  running: "spinner",
  done: "done",
  failed: "failed",
  cancelled: "cancelled",
};

export default function ActivityPanel({ conn }: { conn: Connection }) {
  const [tasks, setTasks] = useState<TaskEntry[]>([]);
  const [mode, setMode] = useState<ConnectionMode>("connecting");
  const [now, setNow] = useState(() => Math.floor(Date.now() / 1000));
  const [selected, setSelected] = useState<number | null>(null);

  useEffect(() => {
    const hydrateInit = async () => {
      try {
        setTasks(await api.tasks());
      } catch {
        setTasks([]);
      }
    };
    const refreshTasks = async () => {
      try {
        setTasks(await api.tasks());
      } catch {}
    };
    void hydrateInit();
    const offMessage = conn.onMessage((message) => {
      if (message.kind === "tasks") setTasks(message.payload as TaskEntry[]);
    });
    const offMode = conn.onModeChange(setMode);
    const clock = window.setInterval(() => setNow(Math.floor(Date.now() / 1000)), 1000);
    const refresh = window.setInterval(() => {
      void refreshTasks();
    }, 5000);
    return () => {
      offMessage();
      offMode();
      window.clearInterval(clock);
      window.clearInterval(refresh);
    };
  }, [conn]);

  const cancel = async (event: MouseEvent, id: number) => {
    event.stopPropagation();
    await api.cancelTask(id).catch(() => undefined);
  };

  const running = tasks.filter((task) => task.status === "running").length;

  return (
    <aside className="activity">
      <div className="activity-head">
        <span className="head-label">Activity Pipeline</span>
        <span className="head-right">
          {running > 0 && <span className="running-pill">{running} running</span>}
          <span className={`conn-dot ${mode}`} title={`connection: ${mode}`} />
        </span>
      </div>
      <div className="activity-sub">
        {running > 0 ? `${running} running` : "idle"}
      </div>
      <ul className="activity-list">
        {tasks.length === 0 && <li className="activity-empty">No background tasks yet.</li>}
        {tasks.map((task) => (
          <li
            key={task.id}
            className={`activity-item ${task.status}`}
            onClick={() => setSelected(task.id)}
            title="View task log"
          >
            <span className={`activity-icon ${STATUS_ICON[task.status] ?? "done"}`} />
            <div className="activity-body">
              <span className="activity-label" title={task.label}>
                {task.label}
              </span>
              <span className="activity-meta">
                <span className="activity-kind">{task.kind}</span>
                <span>{elapsed(task, now)}</span>
              </span>
            </div>
            {task.status === "running" && task.cancellable && (
              <button className="activity-cancel" onClick={(event) => cancel(event, task.id)} title="Cancel">
                ×
              </button>
            )}
          </li>
        ))}
      </ul>
      {selected !== null && (
        <TaskDetail taskId={selected} conn={conn} onClose={() => setSelected(null)} />
      )}
    </aside>
  );
}
