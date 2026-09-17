import { useEffect, useMemo, useState } from "react";
import { Outlet, useOutletContext } from "react-router-dom";
import { Connection } from "./lib/connection";
import { api } from "./lib/api";
import Icon from "./components/Icon";
import ActivityPanel from "./components/ActivityPanel";
import StatusBar from "./components/StatusBar";
import ConfirmHost from "./components/ConfirmHost";
import Sidemenu from "./components/Sidemenu";

export type LayoutContext = { conn: Connection };

export function useConn(): Connection {
  return useOutletContext<LayoutContext>().conn;
}

export default function Layout() {
  const conn = useMemo(() => new Connection(), []);
  const version = import.meta.env.VITE_BITFORGE_VERSION ?? "";
  const [pwd, setPwd] = useState<string>("…");
  const [projectName, setProjectName] = useState<string>("");
  const [branch, setBranch] = useState<string>("");
  const [activityOpen, setActivityOpen] = useState(true);
  const [buildCount, setBuildCount] = useState<number | null>(null);
  const [recipeCount, setRecipeCount] = useState<number | null>(null);
  const [cachePct, setCachePct] = useState<number | null>(null);
  useEffect(() => {
    const hydrateInit = async () => {
      try {
        const info = await api.info();
        setPwd(info.pwd);
      } catch {
        setPwd("");
      }
      try {
        const project = await api.project();
        setProjectName(project.name);
        setBranch(project.yocto_release);
      } catch {}
      try {
        const data = await api.builds();
        setBuildCount(data.history.length);
      } catch {}
      try {
        const tree = await api.deptree();
        setRecipeCount(tree.recipe_count);
      } catch {}
      try {
        const cache = await api.cache();
        if (cache.total_bytes > 0) {
          setCachePct(Math.round((cache.cache_bytes / cache.total_bytes) * 100));
        }
      } catch {}
    };

    void hydrateInit();
    conn.start();
    return () => conn.stop();
  }, [conn]);

  const crumbs = useMemo(() => pwd.split("/").filter(Boolean), [pwd]);

  return (
    <div className="app">
      <nav className="topnav">
        <div className="brand-group">
          <span className="brand-logo">
            <svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
              <path d="M12 2L2 7l10 5 10-5-10-5zM2 17l10 5 10-5M2 12l10 5 10-5" />
            </svg>
          </span>
          <span className="brand">BitForge</span>
        </div>
        <span className="crumb-sep">/</span>
        <div className="breadcrumb" title={pwd}>
          {crumbs.slice(0, -1).map((part, index) => (
            <span key={`${part}-${index}`} className="crumb-item">
              <span className="crumb">{part}</span>
              <span className="crumb-sep"> / </span>
            </span>
          ))}
          <span className="crumb-active">
            {projectName || crumbs[crumbs.length - 1] || "workspace"}
            {branch && (
              <span className="branch-pill">
                <span className="dot" />
                {branch}
              </span>
            )}
          </span>
        </div>
        <div className="topnav-links">
          <a
            href="https://github.com/DraviaVemal/BitForge"
            target="_blank"
            rel="noreferrer"
            title="BitForge on GitHub"
          >
            <Icon name="github" />
          </a>
          <a href="http://docs.draviavemal.com/" target="_blank" rel="noreferrer" title="Documentation">
            <Icon name="docs" />
          </a>
        </div>
      </nav>
      <div className="body">
        <Sidemenu
          projectName={projectName}
          buildCount={buildCount}
          recipeCount={recipeCount}
          cachePct={cachePct}
        />
        <main className="content">
          <Outlet context={{ conn } satisfies LayoutContext} />
        </main>
        {activityOpen && <ActivityPanel conn={conn} />}
      </div>
      <StatusBar
        conn={conn}
        version={version}
        activityOpen={activityOpen}
        onToggleActivity={() => setActivityOpen((open) => !open)}
      />
      <ConfirmHost />
    </div>
  );
}