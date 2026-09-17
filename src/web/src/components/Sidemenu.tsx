import { NavLink } from "react-router-dom";
import Icon, { type IconName } from "./Icon";

type BadgeTone = "default" | "accent" | "cyan";

const MENU: { path: string; label: string; icon: IconName; badge?: "builds" | "recipes" | "cache" }[] = [
  { path: "/", label: "Configuration", icon: "dashboard" },
  { path: "/builds", label: "Build History", icon: "builds", badge: "builds" },
  { path: "/dependency", label: "Dependency", icon: "dependency" },
  { path: "/recipes", label: "Recipe Tree", icon: "recipe", badge: "recipes" },
  { path: "/binaries", label: "Package Binaries", icon: "binary" },
  { path: "/disk", label: "Disk & Layout", icon: "disk" },
  { path: "/environment", label: "Environment", icon: "environment" },
  { path: "/cache", label: "sstate Cache", icon: "cache", badge: "cache" },
];

export type SidemenuProps = {
  projectName: string;
  buildCount: number | null;
  recipeCount: number | null;
  cachePct: number | null;
};

export default function Sidemenu({ projectName, buildCount, recipeCount, cachePct }: SidemenuProps) {
  const badgeFor = (kind?: "builds" | "recipes" | "cache"): { text: string; tone: BadgeTone } | null => {
    if (kind === "builds" && buildCount) return { text: String(buildCount), tone: "default" };
    if (kind === "recipes" && recipeCount) return { text: recipeCount.toLocaleString(), tone: "accent" };
    if (kind === "cache" && cachePct !== null) return { text: `${cachePct}%`, tone: "cyan" };
    return null;
  };

  return (
    <aside className="sidemenu">
      <div className="target-switch">
        <div className="target-switch-inner" title="Active target">
          <span className="target-name">
            <span className="target-swatch" />
            {projectName || "workspace"}
          </span>
          <svg
            className="chev"
            width="14"
            height="14"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth="2"
            strokeLinecap="round"
            strokeLinejoin="round"
          >
            <path d="M19 9l-7 7-7-7" />
          </svg>
        </div>
      </div>
      <ul>
        {MENU.map((item) => {
          const badge = badgeFor(item.badge);
          return (
            <li key={item.path}>
              <NavLink
                to={item.path}
                end={item.path === "/"}
                className={({ isActive }) => (isActive ? "nav-item active" : "nav-item")}
              >
                <Icon name={item.icon} />
                <span>{item.label}</span>
                {badge && <span className={`nav-badge ${badge.tone}`}>{badge.text}</span>}
              </NavLink>
            </li>
          );
        })}
      </ul>
      <div className="sidemenu-foot">
        <div className="foot-row">
          <span className="daemon">
            <span className="dot" /> Daemon OK
          </span>
        </div>
      </div>
    </aside>
  );
}
