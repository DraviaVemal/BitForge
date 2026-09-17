import { useEffect, useState } from "react";
import type {
  BuildSnapshot,
  DepTree,
  ImageRecipe,
  PlanInfo,
  Project as ProjectType,
  SystemInfo,
} from "../lib/api";
import { api } from "../lib/api";
import { formatBytes, formatDuration } from "../lib/format";
import BuildStatus from "../components/BuildStatus";
import TaskTable from "../components/TaskTable";
import { useConn } from "../Layout";

const PACKAGE_CLASSES = ["package_rpm", "package_deb", "package_ipk"];

export default function Dashboard() {
  const conn = useConn();
  const [project, setProject] = useState<ProjectType | null>(null);
  const [images, setImages] = useState<ImageRecipe[]>([]);
  const [machines, setMachines] = useState<string[]>([]);
  const [distros, setDistros] = useState<string[]>([]);
  const [releases, setReleases] = useState<string[]>([]);
  const [imagesNote, setImagesNote] = useState("Loading images from BitBake…");
  const [active, setActive] = useState<BuildSnapshot | null>(null);
  const [insights, setInsights] = useState<DepTree | null>(null);
  const [insightNote, setInsightNote] = useState("Loading build insights…");
  const [plan, setPlan] = useState<PlanInfo | null>(null);
  const [system, setSystem] = useState<SystemInfo | null>(null);
  const [saving, setSaving] = useState(false);
  const [message, setMessage] = useState("");
  const [error, setError] = useState("");
  const [now, setNow] = useState(() => Math.floor(Date.now() / 1000));

  useEffect(() => {
    const timer = window.setInterval(() => setNow(Math.floor(Date.now() / 1000)), 1000);
    return () => window.clearInterval(timer);
  }, []);

  useEffect(() => {
    if (!plan || plan.computed) return;
    const timer = window.setTimeout(() => {
      void (async () => {
        try {
          setPlan(await api.plan());
        } catch {}
      })();
    }, 5000);
    return () => window.clearTimeout(timer);
  }, [plan]);

  useEffect(() => {
    const hydrateInit = async () => {
      try {
        setProject(await api.project());
      } catch (error) {
        setError(String((error as Error).message ?? error));
      }
      try {
        const builds = await api.builds();
        setActive(builds.active);
      } catch {}
      try {
        setSystem(await api.system());
      } catch {}
      try {
        setPlan(await api.plan());
      } catch {}
      try {
        const meta = await api.metadata();
        const images = meta.images ?? [];
        setImages(images);
        setMachines(meta.machines ?? []);
        setDistros(meta.distros ?? []);
        setReleases(meta.releases ?? []);
        setImagesNote(images.length ? "" : "No image recipes found in active layers.");
      } catch (error) {
        setImagesNote(String((error as Error).message ?? error));
      }
      try {
        const tree = await api.deptree();
        if (tree.build_active && tree.recipe_count === undefined) {
          setInsightNote(tree.note ?? "");
        } else {
          setInsights(tree);
          setInsightNote("");
        }
      } catch (error) {
        setInsightNote(String((error as Error).message ?? error));
      }
    };

    void hydrateInit();
    const off = conn.onMessage((message) => {
      if (message.kind === "build") setActive(message.payload as BuildSnapshot);
    });
    return off;
  }, [conn]);

  const running = active?.status === "running";
  const locked = running || saving;

  const field = (key: keyof ProjectType) => (event: { target: { value: string } }) =>
    project && setProject({ ...project, [key]: event.target.value });

  const onBuild = async () => {
    setError("");
    try {
      await api.startBuild();
    } catch (error) {
      setError(String((error as Error).message ?? error));
    }
  };

  const save = async () => {
    if (!project) return;
    setSaving(true);
    setMessage("");
    try {
      const updated = await api.updateProject({
        distro: project.distro,
        machine: project.machine,
        target_image: project.target_image,
        yocto_release: project.yocto_release,
        package_classes: project.package_classes,
        bb_number_threads: project.bb_number_threads,
        parallel_make: project.parallel_make,
        image_fstypes: project.image_fstypes,
      });
      setProject(updated);
      setMessage("Saved. Applies on the next build.");
    } catch (error) {
      setMessage(String((error as Error).message ?? error));
    } finally {
      setSaving(false);
    }
  };

  if (!project) {
    return (
      <section className="page">
        <h1>Project</h1>
        <p className="muted">{error || "Loading…"}</p>
      </section>
    );
  }

  const bbAuto = !project.bb_number_threads;
  const makeAuto = !project.parallel_make;

  return (
    <section className="page">
      <div className="page-head">
        <div className="head-title">
          <h1>{project.name}</h1>
          <span className={`state-pill ${running ? "busy" : "ready"}`}>
            <span className="dot" />
            {running ? "Building" : "Ready to Build"}
          </span>
        </div>
        <button className="build-cta" onClick={onBuild} disabled={running}>
          <svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
            <path d="M8 5v14l11-7z" />
          </svg>
          {running ? "Building…" : "Build Target"}
        </button>
      </div>
      <p className="muted small">
        BitBake {project.bitbake} · settings are written to conf/local.conf and apply on the next build
        {running && " · locked while a build is running"}
      </p>
      {error && <p className="error">{error}</p>}

      {running && (
        <>
          <details className="collapsible">
            <summary>Build details</summary>
            <div className="cards">
              <div className="card"><span>Target image</span><strong>{project.target_image}</strong></div>
              <div className="card"><span>Machine</span><strong>{project.machine}</strong></div>
              <div className="card"><span>Distro</span><strong>{project.distro || "(default)"}</strong></div>
              <div className="card"><span>Release</span><strong>{project.yocto_release}</strong></div>
              <div className="card"><span>Package classes</span><strong>{project.package_classes || "(default)"}</strong></div>
              <div className="card"><span>Image FSTYPES</span><strong>{project.image_fstypes || "(default)"}</strong></div>
              <div className="card">
                <span>BB threads</span>
                <strong>{project.bb_number_threads || (system ? `${system.recommended_bb_threads} (auto)` : "auto")}</strong>
              </div>
              <div className="card">
                <span>Parallel make</span>
                <strong>{project.parallel_make || (system ? `${system.recommended_parallel_make} (auto)` : "auto")}</strong>
              </div>
              <div className="card">
                <span>Execution time</span>
                <strong>{active ? formatDuration((active.finished_at ?? now) - active.started_at) : "—"}</strong>
              </div>
            </div>
            {system && (
              <div className="cards">
                <div className="card"><span>CPUs</span><strong>{system.cpus}</strong></div>
                <div className="card">
                  <span>Effective threads</span>
                  <strong>{system.effective_threads}</strong>
                </div>
                <div className="card"><span>RAM</span><strong>{formatBytes(system.ram_bytes)}</strong></div>
                <div className="card"><span>Swap</span><strong>{formatBytes(system.swap_bytes)}</strong></div>
              </div>
            )}
          </details>

          <h2>Overall build progress</h2>
          {plan?.computed && (
            <div className="cards">
              <div className="card">
                <span>Recipes to rebuild</span>
                <strong className="count-dirty">{(plan.dirty ?? 0).toLocaleString()}</strong>
              </div>
              <div className="card">
                <span>Reusable from cache</span>
                <strong className="count-cache">{(plan.clean ?? 0).toLocaleString()}</strong>
              </div>
              <div className="card">
                <span>Recipes in closure</span>
                <strong>{(plan.total ?? 0).toLocaleString()}</strong>
              </div>
            </div>
          )}
          <BuildStatus active={active} />

          <h2>Tasks</h2>
          <TaskTable active={active} />
        </>
      )}

      {!running && (
      <>
      <h2>Settings</h2>
      <div className="form-grid">
        <label>
          Target image
          <select value={project.target_image} onChange={field("target_image")} disabled={locked}>
            {project.target_image && !images.some((image) => image.name === project.target_image) && (
              <option value={project.target_image}>{project.target_image}</option>
            )}
            {images.map((image) => (
              <option key={image.name} value={image.name}>
                {image.name}
              </option>
            ))}
          </select>
          {imagesNote && <span className="muted small">{imagesNote}</span>}
        </label>
        <label>
          Machine
          <select value={project.machine} onChange={field("machine")} disabled={locked}>
            {project.machine && !machines.includes(project.machine) && (
              <option value={project.machine}>{project.machine}</option>
            )}
            {machines.map((name) => (
              <option key={name} value={name}>
                {name}
              </option>
            ))}
          </select>
        </label>
        <label>
          Distro
          <select value={project.distro} onChange={field("distro")} disabled={locked}>
            <option value="">(default)</option>
            {project.distro && !distros.includes(project.distro) && (
              <option value={project.distro}>{project.distro}</option>
            )}
            {distros.map((name) => (
              <option key={name} value={name}>
                {name}
              </option>
            ))}
          </select>
        </label>
        <label>
          Yocto release
          <select value={project.yocto_release} onChange={field("yocto_release")} disabled={locked}>
            {project.yocto_release && !releases.includes(project.yocto_release) && (
              <option value={project.yocto_release}>{project.yocto_release}</option>
            )}
            {releases.map((name) => (
              <option key={name} value={name}>
                {name}
              </option>
            ))}
          </select>
        </label>
        <label>
          Package classes
          <select value={project.package_classes} onChange={field("package_classes")} disabled={locked}>
            <option value="">(unset)</option>
            {PACKAGE_CLASSES.map((value) => (
              <option key={value} value={value}>
                {value}
              </option>
            ))}
          </select>
        </label>
        <label>
          Image FSTYPES
          <input
            value={project.image_fstypes}
            placeholder="e.g. wic ext4"
            onChange={field("image_fstypes")}
            disabled={locked}
          />
        </label>
        <label>
          BB threads
          <input
            value={project.bb_number_threads}
            placeholder={system ? `${system.recommended_bb_threads} (auto)` : "auto"}
            onChange={field("bb_number_threads")}
            disabled={locked}
          />
        </label>
        <label>
          Parallel make
          <input
            value={project.parallel_make}
            placeholder={system ? `${system.recommended_parallel_make} (auto)` : "auto"}
            onChange={field("parallel_make")}
            disabled={locked}
          />
        </label>
      </div>
      <div className="form-actions">
        <button className="primary" onClick={save} disabled={locked}>
          Save
        </button>
        {message && <span className="muted">{message}</span>}
      </div>

      <h2>System resources &amp; parallelism</h2>
      {system ? (
        <div className="metric-flow">
          <div className="card">
            <span>CPUs</span>
            <strong>{system.cpus}</strong>
          </div>
          <div className="card">
            <span>Effective threads</span>
            <strong>{system.effective_threads}</strong>
            <em className="muted small">{system.reserved_threads} reserved for BitForge</em>
          </div>
          <div className="card">
            <span>RAM</span>
            <strong>{formatBytes(system.ram_bytes)}</strong>
            <em className="muted small">{formatBytes(system.ram_available_bytes)} available</em>
          </div>
          <div className="card">
            <span>Swap</span>
            <strong>{formatBytes(system.swap_bytes)}</strong>
            <em className="muted small">{formatBytes(system.swap_free_bytes)} free</em>
          </div>
          <div className="card">
            <span>Parallel tasks (BB_NUMBER_THREADS)</span>
            <strong>
              {bbAuto ? `${system.recommended_bb_threads}` : project.bb_number_threads}
              {bbAuto && <span className="auto-tag"> (auto)</span>}
            </strong>
            <em className="muted small">written to build/conf/local.conf</em>
          </div>
          <div className="card">
            <span>Parallel make (PARALLEL_MAKE)</span>
            <strong>
              {makeAuto ? system.recommended_parallel_make : project.parallel_make}
              {makeAuto && <span className="auto-tag"> (auto)</span>}
            </strong>
            <em className="muted small">written to build/conf/local.conf</em>
          </div>
        </div>
      ) : (
        <p className="muted small">Reading system resources…</p>
      )}

      <h2>Build insights</h2>
      {insights ? (
        <div className="metric-flow">
          <div className="card">
            <span>Total tasks</span>
            <strong>{insights.total_tasks.toLocaleString()}</strong>
          </div>
          <div className="card">
            <span>Recipes to build</span>
            <strong>{insights.recipe_count.toLocaleString()}</strong>
          </div>
          <div className="card">
            <span>Recipe links</span>
            <strong>{insights.edge_count.toLocaleString()}</strong>
          </div>
          <div className="card">
            <span>Image</span>
            <strong>{insights.image}</strong>
          </div>
        </div>
      ) : (
        <p className="muted small">{insightNote}</p>
      )}

      {active && (
        <>
          <h2>Last build</h2>
          <BuildStatus active={active} />
          <TaskTable active={active} />
        </>
      )}
      </>
      )}
    </section>
  );
}
