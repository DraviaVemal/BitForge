type IconName =
  | "dashboard"
  | "project"
  | "builds"
  | "dependency"
  | "disk"
  | "environment"
  | "cache"
  | "recipe"
  | "binary"
  | "github"
  | "docs";

const PATHS: Record<IconName, string> = {
  dashboard: "M4 4h7v7H4V4zm9 0h7v4h-7V4zm0 6h7v10h-7V10zm-9 3h7v7H4v-7z",
  project: "M4 6h16M4 12h16M4 18h16M8 6v0M14 12v0M10 18v0",
  builds: "M3 7l9-4 9 4-9 4-9-4zm0 5l9 4 9-4M3 17l9 4 9-4",
  dependency: "M6 3v6a3 3 0 003 3h6M6 9a2 2 0 100-4 2 2 0 000 4zm12 6a2 2 0 100-4 2 2 0 000 4zM6 21a2 2 0 100-4 2 2 0 000 4z",
  disk: "M3 6a2 2 0 012-2h14a2 2 0 012 2v12a2 2 0 01-2 2H5a2 2 0 01-2-2V6zm0 5h18M7 15h2",
  environment: "M12 3a9 9 0 100 18 9 9 0 000-18zm0 0c2.5 2.5 2.5 15.5 0 18M3 12h18",
  cache: "M4 6c0-1.7 3.6-3 8-3s8 1.3 8 3-3.6 3-8 3-8-1.3-8-3zm0 0v12c0 1.7 3.6 3 8 3s8-1.3 8-3V6M4 12c0 1.7 3.6 3 8 3s8-1.3 8-3",
  recipe: "M12 3v4m0 0a3 3 0 00-3 3v1H6a2 2 0 00-2 2v6h6v-4m4 4h6v-6a2 2 0 00-2-2h-3v-1a3 3 0 00-3-3z",
  binary: "M4 4h6v6H4V4zm10 0h6v6h-6V4zM4 14h6v6H4v-6zm12 1v4m2-4v4M14 15h6M14 19h6",
  github:
    "M12 2a10 10 0 00-3.2 19.5c.5.1.7-.2.7-.5v-1.7c-2.8.6-3.4-1.3-3.4-1.3-.5-1.2-1.1-1.5-1.1-1.5-.9-.6.1-.6.1-.6 1 .1 1.5 1 1.5 1 .9 1.5 2.3 1.1 2.9.8.1-.6.3-1.1.6-1.4-2.2-.3-4.6-1.1-4.6-5a4 4 0 011-2.7c-.1-.3-.4-1.3.1-2.6 0 0 .8-.3 2.7 1a9.3 9.3 0 015 0c1.9-1.3 2.7-1 2.7-1 .5 1.3.2 2.3.1 2.6a4 4 0 011 2.7c0 3.9-2.3 4.7-4.6 5 .4.3.7.9.7 1.9v2.7c0 .3.2.6.7.5A10 10 0 0012 2z",
  docs: "M6 2h9l5 5v15a1 1 0 01-1 1H6a1 1 0 01-1-1V3a1 1 0 011-1zm9 0v5h5M8 12h8M8 16h8M8 8h3",
};

export default function Icon({ name }: { name: IconName }) {
  return (
    <svg
      className="menu-icon"
      viewBox="0 0 24 24"
      width="18"
      height="18"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.8"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <path d={PATHS[name]} />
    </svg>
  );
}

export type { IconName };
