export type Info = { pwd: string; version: string };

export type CacheEntry = {
  key: string;
  label: string;
  path: string;
  exists: boolean;
  bytes: number;
  files: number;
};

export type CacheInfo = {
  entries: CacheEntry[];
  total_bytes: number;
  downloads_bytes: number;
  cache_bytes: number;
  scanning?: boolean;
  updated_at?: number | null;
};

export type BuildTaskRow = {
  recipe: string;
  task: string;
  status: string;
  started_at?: number | null;
  finished_at?: number | null;
  signature?: string | null;
};

export type Project = {
  name: string;
  distro: string;
  machine: string;
  target_image: string;
  yocto_release: string;
  bitbake: string;
  package_classes: string;
  bb_number_threads: string;
  parallel_make: string;
  image_fstypes: string;
};

export type ImageRecipe = { name: string; recipe_path: string };
export type LiveLayer = { name: string; path: string; priority: number | null };

export type ProjectMetadata = {
  images: ImageRecipe[];
  layers: LiveLayer[];
  machines: string[];
  distros: string[];
  releases: string[];
  environment: Record<string, string>;
  multiconfig: string[];
  bblayers: string[];
  build_active?: boolean;
  note?: string;
};

export type DependencyNode = {
  name: string;
  repository: string;
  path: string;
  git: string;
  branch?: string | null;
  tag?: string | null;
  commit?: string | null;
  priority?: number | null;
  linked: boolean;
};

export type DependencyTree = {
  layers: { name: string; path: string; priority: number }[];
  dependencies: DependencyNode[];
};

export type WksPartition = {
  mountpoint: string | null;
  label: string | null;
  fstype: string | null;
  source: string | null;
  sourceparams: string | null;
  disk: string | null;
  size_kib: number;
  fixed_size_kib: number;
  active: boolean;
  align: number | null;
  no_table: boolean;
  uuid: string | null;
  extra_space_kib: number;
  overhead_factor: number;
};

export type WksStructure = {
  partitions: WksPartition[];
  bootloader: Record<string, unknown>;
  includes: string[];
  expanded_content: string;
};

export type ImageLayout = {
  image: string;
  machine: string;
  recipe_path: string | null;
  appends: string[];
  image_fstypes: string[];
  wic_enabled: boolean;
  wks_file: string;
  candidates: string[];
  search_paths: string[];
  final_path: string | null;
  source_path: string | null;
  template: boolean;
  owning_layer: string | null;
  content: string;
  structure: WksStructure | null;
  parse_error: string | null;
  editable_in_place: boolean;
  project_layers: string[];
  default_layer: string | null;
  bblayers: string[];
  build_active?: boolean;
  note?: string;
};

export type RunningTask = { recipe: string; task: string };
export type TaskGroup = { task: string; running: number };

export type TaskView = {
  total: number;
  done: number;
  planned: number;
  running: number;
  failed: number;
  setscene_total: number;
  setscene_covered: number;
  processing: RunningTask[];
  recent_done: string[];
  groups: TaskGroup[];
};

export type BuildSnapshot = {
  id: number;
  status: string;
  target: string;
  overall_progress: number;
  current_task: string;
  started_at: number;
  finished_at?: number | null;
  tasks: TaskView;
};

export type BuildRecord = {
  id: number;
  target: string;
  status: string;
  started_at: number;
  finished_at?: number | null;
  elapsed_secs?: number | null;
  report_dir?: string | null;
  log_path?: string | null;
};

export type Builds = { active: BuildSnapshot | null; history: BuildRecord[] };

export type TaskEntry = {
  id: number;
  label: string;
  kind: string;
  status: string;
  started_at: number;
  finished_at?: number | null;
  cancellable: boolean;
};

export type TaskDetail = TaskEntry & { log: string[] };

export type DepTree = {
  image: string;
  total_tasks: number;
  recipe_count: number;
  edge_count: number;
  recipes: string[];
  edges: [string, string][];
  build_active?: boolean;
  note?: string;
};

export type RecipeNode = { deps: string[]; status: string; layer: string };

export type RecipeTree = {
  root: string;
  recipes: Record<string, RecipeNode>;
  stats: { total: number; dirty: number; clean: number };
  build_active?: boolean;
  note?: string;
};

export type PlanInfo = {
  image: string;
  computed: boolean;
  dirty?: number;
  clean?: number;
  total?: number;
  build_active?: boolean;
  note?: string;
};

export type ArtifactImage = { name: string; machine: string; bytes: number };
export type ArtifactPackage = { name: string; recipe: string; bytes: number };
export type Artifacts = { images: ArtifactImage[]; packages: ArtifactPackage[] };

export type SystemInfo = {
  cpus: number;
  reserved_threads: number;
  effective_threads: number;
  ram_bytes: number;
  ram_available_bytes: number;
  swap_bytes: number;
  swap_free_bytes: number;
  recommended_bb_threads: number;
  recommended_parallel_make: string;
};

async function getJson<T>(url: string): Promise<T> {
  const response = await fetch(url);
  if (!response.ok) throw new Error(await errorText(response));
  return response.json() as Promise<T>;
}

async function getText(url: string): Promise<string> {
  const response = await fetch(url);
  if (!response.ok) throw new Error(await errorText(response));
  return response.text();
}

async function sendJson<T>(url: string, body?: unknown): Promise<T> {
  const response = await fetch(url, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  if (!response.ok) throw new Error(await errorText(response));
  return response.json() as Promise<T>;
}

async function errorText(response: Response): Promise<string> {
  try {
    const data = await response.json();
    return (data as { error?: string }).error ?? response.statusText;
  } catch {
    return response.statusText;
  }
}

async function getDedupe<T>(url: string): Promise<T> {
  for (let attempt = 0; attempt < 160; attempt += 1) {
    const response = await fetch(url);
    if (response.status === 409) {
      const data = (await response.json().catch(() => ({}))) as { running?: boolean };
      if (data.running) {
        await new Promise((resolve) => setTimeout(resolve, 1500));
        continue;
      }
    }
    if (!response.ok) throw new Error(await errorText(response));
    return response.json() as Promise<T>;
  }
  throw new Error("Timed out waiting for the in-progress activity to finish.");
}

export const api = {
  info: () => getJson<Info>("/api/info"),
  project: () => getJson<Project>("/api/project"),
  updateProject: (patch: Partial<Project>) => sendJson<Project>("/api/project", patch),
  dependencies: () => getJson<DependencyTree>("/api/dependencies"),
  addDependency: (spec: string) =>
    sendJson<DependencyTree>("/api/dependency", { action: "add", spec }),
  removeDependency: (name: string) =>
    sendJson<DependencyTree>("/api/dependency", { action: "remove", name }),
  delinkDependency: (layer: string) =>
    sendJson<DependencyTree>("/api/dependency", { action: "delink", layer }),
  relinkDependency: (layer: string) =>
    sendJson<DependencyTree>("/api/dependency", { action: "relink", layer }),
  builds: () => getJson<Builds>("/api/builds"),
  buildLog: (id: number) => getText(`/api/builds/${id}/log`),
  buildTasks: () => getJson<BuildTaskRow[]>("/api/build/tasks"),
  startBuild: () => sendJson<{ id: number }>("/api/build"),
  cancelBuild: () => sendJson<{ cancelled: boolean }>("/api/build/cancel"),
  metadata: (refresh = false) =>
    getDedupe<ProjectMetadata>(`/api/metadata${refresh ? "?refresh=1" : ""}`),
  layout: (image: string, refresh = false) =>
    getDedupe<ImageLayout>(
      `/api/layout?image=${encodeURIComponent(image)}${refresh ? "&refresh=1" : ""}`,
    ),
  previewLayout: (image: string, content: string) =>
    sendJson<ImageLayout>("/api/layout/preview", { image, content }),
  saveLayout: (image: string, content: string, target_layer?: string) =>
    sendJson<ImageLayout>("/api/layout/save", { image, content, target_layer }),
  tasks: () => getJson<TaskEntry[]>("/api/tasks"),
  task: (id: number) => getJson<TaskDetail>(`/api/tasks/${id}`),
  cancelTask: (id: number) => sendJson<{ cancelled: boolean }>(`/api/tasks/${id}/cancel`),
  deptree: (image?: string) =>
    getDedupe<DepTree>(`/api/deptree${image ? `?image=${encodeURIComponent(image)}` : ""}`),
  recipeTree: (image?: string, refresh = false) => {
    const params = new URLSearchParams();
    if (image) params.set("image", image);
    if (refresh) params.set("refresh", "1");
    const query = params.toString();
    return getJson<RecipeTree>(`/api/recipetree${query ? `?${query}` : ""}`);
  },
  plan: (image?: string) =>
    getJson<PlanInfo>(`/api/plan${image ? `?image=${encodeURIComponent(image)}` : ""}`),
  artifacts: () => getJson<Artifacts>("/api/artifacts"),
  system: () => getJson<SystemInfo>("/api/system"),
  cache: () => getJson<CacheInfo>("/api/cache"),
  clearCache: () => sendJson<{ cleared: string[] }>("/api/cache/clear"),
};

