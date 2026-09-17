import { useEffect, useMemo, useState } from "react";
import type { ProjectMetadata } from "../lib/api";
import { api } from "../lib/api";
import { Kpi, KpiGrid } from "../components/Kpi";
import CopyButton from "../components/CopyButton";
import BuildActiveNote from "../components/BuildActiveNote";

type Category = {
  id: string;
  label: string;
  test: (variableName: string) => boolean;
};

const CATEGORIES: Category[] = [
  { id: "machine", label: "Target & Machine", test: (name) => /^(MACHINE|DISTRO|TUNE|TARGET_|PACKAGE_ARCH)/.test(name) },
  {
    id: "compiler",
    label: "Compiler & Flags",
    test: (name) => /(^CC$|^CXX$|CFLAGS|CXXFLAGS|LDFLAGS|CPPFLAGS|TOOLCHAIN|BUILD_CC)/.test(name),
  },
  { id: "paths", label: "Paths & Dirs", test: (name) => /(_DIR$|_DIR:|DIR$|PATH|TMPDIR|WORKDIR|STAGING)/.test(name) },
  { id: "qa", label: "Licensing & QA", test: (name) => /(LICENSE|WARN_QA|ERROR_QA|^QA_|INSANE)/.test(name) },
];

function splitOverride(variableName: string): { base: string; override: string | null } {
  const separator = variableName.indexOf(":");
  if (separator === -1) return { base: variableName, override: null };
  return { base: variableName.slice(0, separator), override: variableName.slice(separator) };
}

export default function Environment() {
  const [meta, setMeta] = useState<ProjectMetadata | null>(null);
  const [filter, setFilter] = useState("");
  const [category, setCategory] = useState("all");
  const [message, setMessage] = useState("Loading environment from BitBake…");
  const [busy, setBusy] = useState(false);

  const load = async (refresh: boolean) => {
    setBusy(true);
    try {
      const data = await api.metadata(refresh);
      setMeta(data);
      setMessage("");
    } catch (error) {
      setMessage(String((error as Error).message ?? error));
    } finally {
      setBusy(false);
    }
  };

  useEffect(() => {
    load(false);
  }, []);

  const entries = useMemo(() => {
    if (!meta?.environment) return [];
    return Object.entries(meta.environment).sort(([left], [right]) => left.localeCompare(right));
  }, [meta]);

  const categoryCounts = useMemo(() => {
    const counts = new Map<string, number>();
    for (const category of CATEGORIES) {
      counts.set(category.id, 0);
    }
    for (const [variableName] of entries) {
      for (const category of CATEGORIES) {
        if (category.test(variableName)) {
          counts.set(category.id, (counts.get(category.id) ?? 0) + 1);
        }
      }
    }
    return counts;
  }, [entries]);

  const rows = useMemo(() => {
    const needle = filter.trim().toLowerCase();
    const activeCategory = CATEGORIES.find((entry) => entry.id === category);
    return entries.filter(([variableName, variableValue]) => {
      if (activeCategory && !activeCategory.test(variableName)) return false;
      if (!needle) return true;
      return (
        variableName.toLowerCase().includes(needle) || variableValue.toLowerCase().includes(needle)
      );
    });
  }, [entries, filter, category]);

  const overrideCount = useMemo(
    () => entries.filter(([variableName]) => variableName.includes(":")).length,
    [entries],
  );

  return (
    <section className="page">
      <div className="page-head">
        <div className="head-lead">
          <div className="head-title">
            <h1>Environment</h1>
            <span className="pill-tag">bbvars / datastore</span>
          </div>
          <span className="head-desc">
            Live datastore of the active configuration. Cached by configuration signature hash.
          </span>
        </div>
        <button className="build-cta" onClick={() => load(true)} disabled={busy}>
          {busy ? "Refreshing…" : "Refresh Datastore"}
        </button>
      </div>

      {meta?.build_active && <BuildActiveNote note={meta.note} />}

      {meta && (
        <KpiGrid>
          <Kpi
            label="Total variables"
            value={entries.length.toLocaleString()}
            hint="active runtime"
            hintTone="emerald"
          />
          <Kpi
            label="Overrides & modifiers"
            value={overrideCount.toLocaleString()}
            tone="amber"
            hint="machine + recipe"
            hintTone="amber"
          />
          <Kpi label="Active layers" value={(meta.layers?.length ?? 0).toLocaleString()} />
          <Kpi label="Filtered matches" value={rows.length.toLocaleString()} tone="accent" />
        </KpiGrid>
      )}

      {meta && (
        <div className="filter-tabs">
          <button
            className={`arch-tab ${category === "all" ? "active" : ""}`}
            onClick={() => setCategory("all")}
          >
            All ({entries.length.toLocaleString()})
          </button>
          {CATEGORIES.map((entry) => (
            <button
              key={entry.id}
              className={`arch-tab ${category === entry.id ? "active" : ""}`}
              onClick={() => setCategory(entry.id)}
            >
              {entry.label} ({(categoryCounts.get(entry.id) ?? 0).toLocaleString()})
            </button>
          ))}
        </div>
      )}

      <div className="add-row">
        <input
          value={filter}
          onChange={(event) => setFilter(event.target.value)}
          placeholder="Filter variables (e.g. MACHINE, DISTRO, IMAGE_INSTALL, CC, CFLAGS, QA)…"
        />
      </div>

      {message && <p className="muted">{message}</p>}

      {meta && (
        <table className="table env-table">
          <thead>
            <tr>
              <th>Variable name</th>
              <th>Value / datastore content</th>
              <th className="env-actions" />
            </tr>
          </thead>
          <tbody>
            {rows.map(([variableName, variableValue]) => {
              const { base, override } = splitOverride(variableName);
              return (
                <tr key={variableName}>
                  <td className="env-key">
                    {base}
                    {override && <span className="override-chip">{override}</span>}
                  </td>
                  <td className="env-value">
                    {variableValue.trim() === "" ? (
                      <span className="value-chip empty">"" (empty string)</span>
                    ) : (
                      variableValue
                    )}
                  </td>
                  <td className="env-actions">
                    <CopyButton value={variableValue} title="Copy value" />
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      )}
      {meta && rows.length === 0 && <p className="muted small">No variables match the filter.</p>}
    </section>
  );
}
