import { useEffect, useState } from "react";
import { useNavigate } from "react-router-dom";
import type { DependencyNode, DependencyTree } from "../lib/api";
import { api } from "../lib/api";
import { shortSha } from "../lib/format";
import CopyButton from "../components/CopyButton";

function referenceOf(dependency: DependencyNode): string {
  return dependency.branch || dependency.tag || dependency.commit || "latest";
}

export default function Dependency() {
  const navigate = useNavigate();
  const openLayerRecipes = (layer: string) =>
    navigate(`/recipes?layer=${encodeURIComponent(layer)}`);
  const [tree, setTree] = useState<DependencyTree | null>(null);
  const [spec, setSpec] = useState("");
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState("");

  const load = async () => {
    try {
      setTree(await api.dependencies());
    } catch (error) {
      setMessage(String((error as Error).message ?? error));
    }
  };

  useEffect(() => {
    void load();
  }, []);

  const run = async (label: string, action: () => Promise<DependencyTree>) => {
    setBusy(true);
    setMessage(label);
    try {
      setTree(await action());
      setMessage("");
    } catch (error) {
      setMessage(String((error as Error).message ?? error));
    } finally {
      setBusy(false);
    }
  };

  const addLayer = async () => {
    const trimmed = spec.trim();
    if (!trimmed) return;
    await run("Cloning layer…", () => api.addDependency(trimmed));
    setSpec("");
  };

  const repositoryCount = tree
    ? new Set(tree.dependencies.map((dependency) => dependency.repository)).size
    : 0;

  return (
    <section className="page">
      <div className="page-head">
        <div className="head-lead">
          <div className="head-title">
            <h1>Dependency Management</h1>
            <span className="pill-tag">bblayers.conf</span>
          </div>
          <span className="head-desc">
            Layer resolution, upstream Git repository mapping, and cross-layer priority management.
          </span>
        </div>
        {tree && (
          <div className="head-stats">
            <span className="head-stat">
              Active Layers<em>{tree.layers.length}</em>
            </span>
            <span className="head-stat">
              Repositories<em>{repositoryCount}</em>
            </span>
          </div>
        )}
      </div>

      <div className="dep-console">
        <div className="dep-input">
          <div className="dep-field">
            <span className="prefix">git://</span>
            <input
              placeholder="git.yoctoproject.org/meta-arm;branch=master#meta-arm"
              value={spec}
              onChange={(event) => setSpec(event.target.value)}
              disabled={busy}
            />
            <span className="uri-tag">URI SPEC</span>
          </div>
          <button className="build-cta" onClick={addLayer} disabled={busy}>
            Add layer
          </button>
        </div>
        <div className="dep-hint">
          <span>
            Syntax: <span className="hint-code">&lt;url&gt;[@branch|commit|tag][:subpath][#dest]</span>
          </span>
        </div>
      </div>

      {message && <p className="muted">{message}</p>}

      {tree && (
        <>
          <div className="section-bar">
            <span className="section-title amber">Project Layers (Local Workspace)</span>
            <span className="section-meta">Higher priority overrides upstream recipes</span>
          </div>
          <table className="table dep-table">
            <thead>
              <tr>
                <th>Priority</th>
                <th>Layer</th>
                <th>Path</th>
                <th>Source Type</th>
                <th className="num">Recipes</th>
              </tr>
            </thead>
            <tbody>
              {tree.layers.map((projectLayer) => (
                <tr key={projectLayer.name}>
                  <td className="count-dirty">{projectLayer.priority}</td>
                  <td>
                    <code>{projectLayer.name}</code>
                  </td>
                  <td className="muted small">{projectLayer.path}</td>
                  <td>
                    <span className="source-type">
                      <span className="swatch" />
                      Local Workspace
                    </span>
                  </td>
                  <td className="num">
                    <button
                      className="ghost-btn"
                      title={`Show recipes provided by ${projectLayer.name}`}
                      onClick={() => openLayerRecipes(projectLayer.name)}
                    >
                      Details
                    </button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>

          <div className="section-bar">
            <span className="section-title">Dependency Layers (Upstream Remote)</span>
          </div>
          {tree.dependencies.length === 0 ? (
            <p className="muted">No external layers.</p>
          ) : (
            <table className="table dep-table">
              <thead>
                <tr>
                  <th>Priority</th>
                  <th>Layer</th>
                  <th>Repository / Remote URI</th>
                  <th>Commit</th>
                  <th>Ref</th>
                  <th>Status</th>
                  <th className="num">Actions</th>
                </tr>
              </thead>
              <tbody>
                {tree.dependencies.map((dependency) => (
                  <tr key={dependency.path} className={dependency.linked ? "" : "delinked"}>
                    <td>{dependency.priority ?? "—"}</td>
                    <td>
                      <code>{dependency.name}</code>
                    </td>
                    <td>
                      <div className="repo-cell">
                        <span className="repo-name">{dependency.repository}</span>
                        {dependency.git && (
                          <a
                            className="repo-url"
                            href={dependency.git}
                            target="_blank"
                            rel="noreferrer"
                          >
                            {dependency.git}
                          </a>
                        )}
                      </div>
                    </td>
                    <td>
                      {dependency.commit ? (
                        <span className="sha-chip">
                          <span className="sha">{shortSha(dependency.commit)}</span>
                          <CopyButton value={dependency.commit} title="Copy SHA" />
                        </span>
                      ) : (
                        <span className="muted">—</span>
                      )}
                    </td>
                    <td>
                      <span className="ref-chip">{referenceOf(dependency)}</span>
                    </td>
                    <td>
                      <span className={`link-status ${dependency.linked ? "" : "off"}`}>
                        <span className="dot" />
                        {dependency.linked ? "linked" : "delinked"}
                      </span>
                    </td>
                    <td>
                      <div className="dep-actions">
                        {dependency.linked ? (
                          <button
                            className="link"
                            disabled={busy}
                            title="Keep the layer on disk, remove just this layer from bblayers.conf"
                            onClick={() =>
                              run(`Delinking ${dependency.name}…`, () =>
                                api.delinkDependency(dependency.path),
                              )
                            }
                          >
                            delink
                          </button>
                        ) : (
                          <button
                            className="link"
                            disabled={busy}
                            onClick={() =>
                              run(`Relinking ${dependency.name}…`, () =>
                                api.relinkDependency(dependency.path),
                              )
                            }
                          >
                            relink
                          </button>
                        )}
                        <button
                          className="ghost-btn"
                          title={`Show recipes provided by ${dependency.name}`}
                          onClick={() => openLayerRecipes(dependency.name)}
                        >
                          details
                        </button>
                        <button
                          className="ghost-btn danger"
                          disabled={busy}
                          title={`Remove the whole ${dependency.repository} repository and delete the checkout`}
                          onClick={() =>
                            run(`Removing ${dependency.repository}…`, () =>
                              api.removeDependency(dependency.repository),
                            )
                          }
                        >
                          remove repo
                        </button>
                      </div>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}

          <div className="section-note">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" aria-hidden="true">
              <path
                d="M13 16h-1v-4h-1m1-4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z"
                strokeLinecap="round"
                strokeLinejoin="round"
              />
            </svg>
            <div>
              <strong>Yocto Project Compatibility Series</strong>
              Configured layers are resolved against <code>LAYERSERIES_COMPAT</code> for the active
              release. Layer priority ordering is applied on the next parse.
            </div>
          </div>
        </>
      )}
    </section>
  );
}
