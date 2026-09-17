import { useEffect, useMemo, useRef, useState } from "react";
import { useSearchParams } from "react-router-dom";
import type { BuildSnapshot, RecipeTree as RecipeTreeData } from "../lib/api";
import { api } from "../lib/api";
import DataTable, { type Column } from "../components/DataTable";
import BuildActiveNote from "../components/BuildActiveNote";
import { useConn } from "../Layout";

type RecipeRow = {
  name: string;
  deps: string[];
  status: string;
  layer: string;
};

type DependencyRow = {
  name: string;
  status: string;
  layer: string;
};

const STATUS_LABEL: Record<string, string> = {
  dirty: "dirty",
  cached: "cached",
  built: "built",
  running: "building",
  failed: "failed",
};

const DIRTY_STATES = ["dirty", "built", "running", "failed"];

function statusCell(status: string) {
  return (
    <span className={`recipe-status status-${status || "unknown"}`}>
      {STATUS_LABEL[status] ?? "—"}
    </span>
  );
}

function layerCell(layer: string) {
  return layer ? <span className="layer-chip">{layer}</span> : <span className="muted">—</span>;
}

export default function RecipeTree() {
  const conn = useConn();
  const [searchParams, setSearchParams] = useSearchParams();
  const layerFilter = searchParams.get("layer");
  const onClearLayerFilter = () => {
    const next = new URLSearchParams(searchParams);
    next.delete("layer");
    setSearchParams(next, { replace: true });
  };
  const [tree, setTree] = useState<RecipeTreeData | null>(null);
  const [note, setNote] = useState("Computing recipe tree… this runs bitbake -g and may take a moment.");
  const [busy, setBusy] = useState(false);
  const [onlyDirty, setOnlyDirty] = useState(false);
  const [selected, setSelected] = useState<string | null>(null);
  const [timeoutHelp, setTimeoutHelp] = useState(false);
  const timeoutAttempts = useRef(0);

  const load = async (refresh = false) => {
    setBusy(true);
    setNote(refresh ? "Recomputing recipe tree…" : "Computing recipe tree…");
    try {
      const data = await api.recipeTree(undefined, refresh);
      setTree(data);
      setNote("");
      timeoutAttempts.current = 0;
      setTimeoutHelp(false);
    } catch (error) {
      const message = String((error as Error).message ?? error);
      setNote(message);
      if (/timeout|timed out/i.test(message)) {
        timeoutAttempts.current += 1;
        if (timeoutAttempts.current >= 2) setTimeoutHelp(true);
      }
    } finally {
      setBusy(false);
    }
  };

  useEffect(() => {
    void load(false);
    const off = conn.onMessage((message) => {
      if (message.kind === "build") {
        const snapshot = message.payload as BuildSnapshot;
        if (snapshot.status !== "running") void load(false);
      }
    });
    return off;
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [conn]);

  const rows = useMemo<RecipeRow[]>(() => {
    if (!tree) return [];
    return Object.entries(tree.recipes)
      .filter(([, node]) => {
        if (onlyDirty && !DIRTY_STATES.includes(node.status)) return false;
        if (layerFilter && node.layer !== layerFilter) return false;
        return true;
      })
      .map(([name, node]) => ({ name, deps: node.deps, status: node.status, layer: node.layer }));
  }, [tree, onlyDirty, layerFilter]);

  const dependencyRows = useMemo<DependencyRow[]>(() => {
    if (!tree || !selected) return [];
    const node = tree.recipes[selected];
    if (!node) return [];
    return node.deps.map((dependencyName) => {
      const dependency = tree.recipes[dependencyName];
      return {
        name: dependencyName,
        status: dependency?.status ?? "",
        layer: dependency?.layer ?? "",
      };
    });
  }, [tree, selected]);

  const columns: Column<RecipeRow>[] = [
    {
      key: "name",
      header: "Recipe",
      type: "text",
      render: (row) => <code>{row.name}</code>,
      value: (row) => row.name,
    },
    {
      key: "layer",
      header: "Layer",
      type: "text",
      render: (row) => layerCell(row.layer),
      value: (row) => row.layer,
    },
    {
      key: "status",
      header: "Status",
      type: "text",
      render: (row) => statusCell(row.status),
      value: (row) => STATUS_LABEL[row.status] ?? "",
    },
    {
      key: "deps",
      header: "Depends on",
      type: "number",
      align: "right",
      value: (row) => row.deps.length,
      render: (row) =>
        row.deps.length > 0 ? (
          <button className="link" onClick={() => setSelected(row.name)}>
            {row.deps.length}
          </button>
        ) : (
          <span className="muted">0</span>
        ),
    },
  ];

  const dependencyColumns: Column<DependencyRow>[] = [
    {
      key: "name",
      header: "Recipe",
      type: "text",
      render: (row) => <code>{row.name}</code>,
      value: (row) => row.name,
    },
    {
      key: "layer",
      header: "Pulled from (layer)",
      type: "text",
      render: (row) => layerCell(row.layer),
      value: (row) => row.layer,
    },
    {
      key: "status",
      header: "Status",
      type: "text",
      render: (row) => statusCell(row.status),
      value: (row) => STATUS_LABEL[row.status] ?? "",
    },
  ];

  const needsImage = !tree && /no image|default image/i.test(note);

  return (
    <section className="page">
      <div className="page-head">
        <h1>Recipe Tree</h1>
        <button className="primary" onClick={() => load(true)} disabled={busy}>
          {busy ? "Working…" : "Recompute"}
        </button>
      </div>
      {tree && (
        <p className="muted small">
          Rooted at <code>{tree.root}</code> · full dependency closure down to poky
        </p>
      )}
      {tree?.build_active && <BuildActiveNote note={tree.note} />}
      {note && !needsImage && <p className="muted">{note}</p>}
      {needsImage && (
        <div className="section-note">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" aria-hidden="true">
            <path
              d="M13 16h-1v-4h-1m1-4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z"
              strokeLinecap="round"
              strokeLinejoin="round"
            />
          </svg>
          <div>
            <strong>No target image selected</strong>
            Open the <strong>Configuration</strong> tab, choose a <code>Target image</code> from the
            dropdown, and click <strong>Save</strong>. Then return here and press{" "}
            <strong>Recompute</strong>.
          </div>
        </div>
      )}      {timeoutHelp && (
        <div className="section-note">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" aria-hidden="true">
            <path
              d="M12 8v4l3 3m6-3a9 9 0 11-18 0 9 9 0 0118 0z"
              strokeLinecap="round"
              strokeLinejoin="round"
            />
          </svg>
          <div>
            <strong>This activity keeps timing out</strong>
            Add <code>activity_timeout = 6</code> under <code>[server]</code> in{" "}
            <code>BitForge.toml</code> (value in minutes) to wait longer, then press{" "}
            <strong>Recompute</strong>.
          </div>
        </div>
      )}
      {tree && (
        <>
          <div className="cards">
            <div className="card">
              <span>Total recipes</span>
              <strong>{tree.stats.total.toLocaleString()}</strong>
            </div>
            <div className="card">
              <span>Needs rebuild</span>
              <strong className="count-dirty">{tree.stats.dirty.toLocaleString()}</strong>
            </div>
            <div className="card">
              <span>Reusable (cache)</span>
              <strong className="count-cache">{tree.stats.clean.toLocaleString()}</strong>
            </div>
          </div>

          <div className="table-toolbar">
            {layerFilter && (
              <span className="filter-chip">
                Layer: <strong>{layerFilter}</strong>
                <button className="chip-clear" onClick={onClearLayerFilter} title="Clear layer filter">
                  ×
                </button>
              </span>
            )}
            <label className="checkbox">
              <input
                type="checkbox"
                checked={onlyDirty}
                onChange={(event) => setOnlyDirty(event.target.checked)}
              />
              Only rebuilds
            </label>
            <span className="muted small">
              {rows.length.toLocaleString()} of {tree.stats.total.toLocaleString()} recipes
            </span>
          </div>

          <DataTable
            columns={columns}
            rows={rows}
            rowKey={(row) => row.name}
            rowClassName={(row) => (row.status === "dirty" ? "row-dirty" : "")}
            initialSort={{ key: "deps", direction: "desc" }}
            empty="No recipes match the filters."
          />
        </>
      )}

      {selected && (
        <div className="modal-overlay" onClick={() => setSelected(null)}>
          <div className="modal task-modal" onClick={(event) => event.stopPropagation()}>
            <div className="modal-head">
              <strong>
                <code>{selected}</code> depends on {dependencyRows.length} recipe(s)
              </strong>
              <button className="ghost-btn" onClick={() => setSelected(null)}>
                Close
              </button>
            </div>
            <p className="muted small">Direct dependencies and the layer each one is pulled from.</p>
            <DataTable
              columns={dependencyColumns}
              rows={dependencyRows}
              rowKey={(row) => row.name}
              initialSort={{ key: "name", direction: "asc" }}
              empty="No direct dependencies."
            />
          </div>
        </div>
      )}
    </section>
  );
}
