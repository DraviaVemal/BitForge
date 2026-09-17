import type { BuildSnapshot, TaskView } from "../lib/api";
import { api } from "../lib/api";
import { confirmDialog } from "../lib/confirm";

const EMPTY_TASKS: TaskView = {
  total: 0,
  done: 0,
  planned: 0,
  running: 0,
  failed: 0,
  setscene_total: 0,
  setscene_covered: 0,
  processing: [],
  recent_done: [],
  groups: [],
};

export default function BuildStatus({ active }: { active: BuildSnapshot | null }) {
  if (!active || active.status === "idle") {
    return <div className="build-status idle">No active build.</div>;
  }
  const running = active.status === "running";
  const tasks = active.tasks ?? EMPTY_TASKS;
  const cacheHitPct =
    tasks.setscene_total > 0
      ? Math.round((tasks.setscene_covered / tasks.setscene_total) * 100)
      : null;

  const cancel = () => {
    void (async () => {
      const ok = await confirmDialog("Cancel the running build? Work already cached is kept.", {
        title: "Cancel build",
        confirmText: "Cancel build",
        cancelText: "Keep building",
        danger: true,
      });
      if (!ok) return;
      try {
        await api.cancelBuild();
      } catch {}
    })();
  };

  return (
    <div className="build-bar">
      <div className="build-bar-info">
        <div className={`build-bar-title status--${active.status}`}>
          #{active.id} · {active.status} · {active.target}
        </div>
        <div className="current-task">{active.current_task}</div>
        <div className="counts">
          <span>{tasks.done.toLocaleString()} completed</span>
          <span>{tasks.running.toLocaleString()} running</span>
          <span>{tasks.planned.toLocaleString()} planned</span>
          <span>{tasks.total.toLocaleString()} total</span>
          {tasks.failed > 0 && <span className="count-failed">{tasks.failed} failed</span>}
          {tasks.setscene_total > 0 && (
            <span>
              sstate {tasks.setscene_covered.toLocaleString()}/{tasks.setscene_total.toLocaleString()}
            </span>
          )}
          {cacheHitPct !== null && <span className="count-cache">cache hit {cacheHitPct}%</span>}
        </div>
        {tasks.groups.length > 0 && (
          <div className="task-groups">
            {tasks.groups.map((group) => (
              <span key={group.task} className="task-group">
                {group.task} <strong>{group.running}</strong>
              </span>
            ))}
          </div>
        )}
      </div>
      <div className="build-bar-progress">
        <span className="pct">{active.overall_progress}%</span>
        <div className="bar">
          <div className="fill" style={{ width: `${active.overall_progress}%` }} />
        </div>
        {running && (
          <button className="cancel" onClick={cancel}>
            Cancel
          </button>
        )}
      </div>
    </div>
  );
}
