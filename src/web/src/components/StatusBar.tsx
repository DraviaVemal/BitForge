import { useEffect, useState } from "react";
import type { Connection } from "../lib/connection";
import type { BuildSnapshot, TaskEntry } from "../lib/api";
import { api } from "../lib/api";

export default function StatusBar({
  conn,
  version,
  activityOpen,
  onToggleActivity,
}: {
  conn: Connection;
  version: string;
  activityOpen: boolean;
  onToggleActivity: () => void;
}) {
  const [tasks, setTasks] = useState<TaskEntry[]>([]);
  const [build, setBuild] = useState<BuildSnapshot | null>(null);

  useEffect(() => {
    const refresh = async () => {
      try {
        setTasks(await api.tasks());
      } catch {}
      try {
        const builds = await api.builds();
        setBuild(builds.active);
      } catch {}
    };
    void refresh();
    const off = conn.onMessage((message) => {
      if (message.kind === "tasks") setTasks(message.payload as TaskEntry[]);
      if (message.kind === "build") setBuild(message.payload as BuildSnapshot);
    });
    const timer = window.setInterval(() => {
      void refresh();
    }, 5000);
    return () => {
      off();
      window.clearInterval(timer);
    };
  }, [conn]);

  const running = tasks.filter((task) => task.status === "running").length;
  const building = build?.status === "running";
  const status =
    running > 0
      ? `Syncing build-system data & validations… (${running} running)`
      : "Up to date — metadata, dependency tree and validations ready";

  return (
    <footer className="statusbar">
      <div className="status-left">
        <span className="status-version">BitForge {version || "—"}</span>
        {building && build && (
          <span className="status-build" title={build.current_task}>
            Building {build.target} · {build.overall_progress}% · {build.current_task}
          </span>
        )}
      </div>
      <div className="status-right">
        <span className="status-text">{status}</span>
        <span className={`status-dot ${running > 0 ? "busy" : "ok"}`} />
        <button className="status-toggle" onClick={onToggleActivity}>
          {activityOpen ? "Hide" : "Show"} activity
          {running > 0 ? ` (${running})` : ""}
        </button>
      </div>
    </footer>
  );
}
