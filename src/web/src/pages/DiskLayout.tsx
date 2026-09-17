import { useEffect, useMemo, useState } from "react";
import type { ImageLayout, ImageRecipe, WksPartition } from "../lib/api";
import { api } from "../lib/api";
import BuildActiveNote from "../components/BuildActiveNote";

const SEGMENT_COLORS = ["#f5a623", "#3fb950", "#58a6ff", "#bc8cff", "#f778ba", "#e3b341"];

const partitionSize = (part: WksPartition) => part.fixed_size_kib || part.size_kib || 0;

const KNOWN_BOOTLOADER_SOURCES = [
  "bootimg-efi",
  "bootimg-pcbios",
  "bootimg-biosplusefi",
  "isoimage-isohybrid",
];

const KNOWN_PTABLES = [
  { value: "msdos", label: "msdos (MBR)" },
  { value: "gpt", label: "gpt" },
];

const BOOTLOADER_VALUE_FLAGS = new Set(["ptable", "timeout", "source", "configfile", "append"]);

type BootloaderForm = {
  ptable: string;
  timeout: string;
  source: string;
  configfile: string;
  append: string;
  extra: string;
};

const EMPTY_BOOTLOADER: BootloaderForm = {
  ptable: "",
  timeout: "",
  source: "",
  configfile: "",
  append: "",
  extra: "",
};

function tokenizeBootloader(input: string): string[] {
  const tokens: string[] = [];
  let index = 0;
  while (index < input.length) {
    while (index < input.length && /\s/.test(input[index])) index += 1;
    if (index >= input.length) break;
    let token = "";
    while (index < input.length && !/\s/.test(input[index])) {
      const char = input[index];
      if (char === '"' || char === "'") {
        index += 1;
        while (index < input.length && input[index] !== char) {
          token += input[index];
          index += 1;
        }
        if (index < input.length) index += 1;
      } else {
        token += char;
        index += 1;
      }
    }
    tokens.push(token);
  }
  return tokens;
}

function bootloaderLineIndex(content: string): number {
  return content
    .split(/\r?\n/)
    .findIndex((line) => /^\s*bootloader\b/.test(line) && !/^\s*#/.test(line));
}

function parseBootloader(content: string): BootloaderForm {
  const line = content
    .split(/\r?\n/)
    .find((entry) => /^\s*bootloader\b/.test(entry) && !/^\s*#/.test(entry));
  const form: BootloaderForm = { ...EMPTY_BOOTLOADER };
  if (!line) return form;
  const tokens = tokenizeBootloader(line.replace(/^\s*bootloader\s*/, ""));
  const leftover: string[] = [];
  for (let index = 0; index < tokens.length; index += 1) {
    const token = tokens[index];
    if (!token.startsWith("--")) {
      leftover.push(token);
      continue;
    }
    const equals = token.indexOf("=");
    let key: string;
    let value: string;
    if (equals !== -1) {
      key = token.slice(2, equals);
      value = token.slice(equals + 1);
    } else {
      key = token.slice(2);
      if (BOOTLOADER_VALUE_FLAGS.has(key)) {
        value = tokens[index + 1] ?? "";
        index += 1;
      } else {
        value = "";
      }
    }
    switch (key) {
      case "ptable":
        form.ptable = value;
        break;
      case "timeout":
        form.timeout = value;
        break;
      case "source":
        form.source = value;
        break;
      case "configfile":
        form.configfile = value;
        break;
      case "append":
        form.append = value;
        break;
      default:
        leftover.push(value ? `${token} ${value}` : token);
    }
  }
  form.extra = leftover.join(" ").trim();
  return form;
}

function composeBootloaderLine(form: BootloaderForm): string {
  const parts = ["bootloader"];
  if (form.ptable) parts.push(`--ptable ${form.ptable}`);
  if (form.timeout.trim()) parts.push(`--timeout ${form.timeout.trim()}`);
  if (form.source.trim()) parts.push(`--source ${form.source.trim()}`);
  if (form.configfile.trim()) parts.push(`--configfile ${form.configfile.trim()}`);
  if (form.append.trim()) {
    parts.push(`--append "${form.append.trim().replace(/"/g, '\\"')}"`);
  }
  if (form.extra.trim()) parts.push(form.extra.trim());
  return parts.join(" ");
}

function applyBootloaderToContent(content: string, form: BootloaderForm): string {
  const composed = composeBootloaderLine(form);
  const lines = content.split(/\r?\n/);
  const index = bootloaderLineIndex(content);
  if (index !== -1) {
    lines[index] = composed;
    return lines.join("\n");
  }
  const trimmed = content.replace(/\s*$/, "");
  return trimmed ? `${trimmed}\n${composed}\n` : `${composed}\n`;
}

export default function DiskLayout() {
  const [images, setImages] = useState<ImageRecipe[]>([]);
  const [image, setImage] = useState<string>("");
  const [layout, setLayout] = useState<ImageLayout | null>(null);
  const [draft, setDraft] = useState<string>("");
  const [bootloader, setBootloader] = useState<BootloaderForm>(EMPTY_BOOTLOADER);
  const [targetLayer, setTargetLayer] = useState<string>("");
  const [message, setMessage] = useState<string>("");
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    const hydrateInit = async () => {
      try {
        const meta = await api.metadata();
        setImages(meta.images);
        setMessage(meta.images.length ? "" : "No image recipes found in active layers.");
      } catch (error) {
        setMessage(String((error as Error).message ?? error));
      }
    };
    void hydrateInit();
  }, []);

  const loadImage = async (name: string) => {
    setImage(name);
    setLayout(null);
    if (!name) return;
    setBusy(true);
    setMessage(`Resolving WKS for ${name}…`);
    try {
      const resolved = await api.layout(name);
      setLayout(resolved);
      setDraft(resolved.content);
      setBootloader(parseBootloader(resolved.content));
      setTargetLayer(resolved.default_layer ?? "");
      setMessage(resolved.parse_error ?? "");
    } catch (error) {
      setMessage(String((error as Error).message ?? error));
    } finally {
      setBusy(false);
    }
  };

  const preview = async () => {
    if (!layout) return;
    setBusy(true);
    try {
      const parsed = await api.previewLayout(layout.image, draft);
      setLayout({ ...layout, structure: parsed.structure, parse_error: parsed.parse_error });
      setMessage(parsed.parse_error ?? "Preview parsed.");
    } catch (error) {
      setMessage(String((error as Error).message ?? error));
    } finally {
      setBusy(false);
    }
  };

  const save = async () => {
    if (!layout) return;
    setBusy(true);
    setMessage("Saving…");
    try {
      const target = layout.editable_in_place ? undefined : targetLayer || undefined;
      const saved = await api.saveLayout(layout.image, draft, target);
      setLayout(saved);
      setDraft(saved.content);
      setBootloader(parseBootloader(saved.content));
      setTargetLayer(saved.default_layer ?? "");
      setMessage(
        saved.editable_in_place
          ? `Saved to ${saved.owning_layer ?? "project layer"}.`
          : "Saved into project layer and set WKS_FILE.",
      );
    } catch (error) {
      setMessage(String((error as Error).message ?? error));
    } finally {
      setBusy(false);
    }
  };

  const setBootloaderField =
    (key: keyof BootloaderForm) => (event: { target: { value: string } }) =>
      setBootloader((current) => ({ ...current, [key]: event.target.value }));

  const applyBootloader = () => {
    setDraft((current) => applyBootloaderToContent(current, bootloader));
    setMessage("Bootloader line written into the WKS source below. Review and Save to persist.");
  };

  const syncBootloader = () => {
    setBootloader(parseBootloader(draft));
    setMessage("Bootloader form re-read from the current WKS source.");
  };

  const totalFixed = useMemo(() => {
    const parts = layout?.structure?.partitions ?? [];
    const sum = parts.reduce((acc, part) => acc + partitionSize(part), 0);
    return Math.max(1, sum);
  }, [layout]);

  return (
    <section className="page">
      <div className="page-head">
        <h1>Disk Layout</h1>
        {layout && (layout.editable_in_place || layout.source_path) && (
          <button className="primary" onClick={save} disabled={busy}>
            {layout.editable_in_place ? "Save in place" : "Save to project layer"}
          </button>
        )}
      </div>

      <label className="inline">
        Image
        <select value={image} onChange={(event) => loadImage(event.target.value)} disabled={busy}>
          <option value="">Select an image…</option>
          {images.map((item) => (
            <option key={item.name} value={item.name}>
              {item.name}
            </option>
          ))}
        </select>
      </label>

      {layout?.build_active && <BuildActiveNote note={layout.note} />}
      {message && <p className="muted">{message}</p>}

      {layout && (
        <>
          <div className="wks-provenance">
            <div>
              <span>WIC enabled</span>
              <strong>{layout.wic_enabled ? "yes" : "no"}</strong>
            </div>
            <div>
              <span>Owning layer</span>
              <strong>{layout.owning_layer ?? "—"}</strong>
            </div>
            <div>
              <span>Origin</span>
              <strong>{layout.editable_in_place ? "project layer" : "inherited"}</strong>
            </div>
            <div>
              <span>Template</span>
              <strong>{layout.template ? ".wks.in" : ".wks"}</strong>
            </div>
          </div>

          <p className="muted small">
            Source: <code>{layout.source_path ?? "unresolved"}</code>
            {layout.final_path && layout.final_path !== layout.source_path && (
              <>
                {" "}· Final: <code>{layout.final_path}</code>
              </>
            )}
          </p>

          {layout.structure && layout.structure.partitions.length > 0 && (
            <>
              <div className="disk-graph">
                {layout.structure.partitions.map((part, index) => {
                  const size = partitionSize(part);
                  return (
                    <div
                      key={index}
                      className="seg"
                      style={{
                        background: SEGMENT_COLORS[index % SEGMENT_COLORS.length],
                        flexGrow: size ? 0 : 1,
                        flexBasis: size ? `${Math.max(6, (size / totalFixed) * 100)}%` : "auto",
                      }}
                      title={`${part.mountpoint ?? part.source ?? "part"} · ${size ? `${Math.round(size / 1024)} MB` : "grow"}`}
                    >
                      {part.mountpoint ?? part.source ?? part.label ?? "part"}
                    </div>
                  );
                })}
              </div>

              <table className="table">
                <thead>
                  <tr>
                    <th>Mount</th>
                    <th>FS</th>
                    <th>Source</th>
                    <th>Size</th>
                    <th>Label</th>
                  </tr>
                </thead>
                <tbody>
                  {layout.structure.partitions.map((part, index) => {
                    const size = partitionSize(part);
                    return (
                      <tr key={index}>
                        <td>{part.mountpoint ?? "—"}</td>
                        <td>{part.fstype ?? "—"}</td>
                        <td>{part.source ?? "—"}</td>
                        <td>{size ? `${Math.round(size / 1024)} MB` : "grow"}</td>
                        <td>{part.label ?? "—"}</td>
                      </tr>
                    );
                  })}
                </tbody>
              </table>
            </>
          )}

          {layout.structure && layout.structure.includes.length > 0 && (
            <p className="muted small">Includes: {layout.structure.includes.join(", ")}</p>
          )}

          {layout.source_path && (
            <>
              <h2>Bootloader</h2>
              <p className="muted small">
                Pick known options from the lists or type your own flags. Apply writes the{" "}
                <code>bootloader</code> line into the WKS source for <code>{layout.image}</code>.
              </p>
              <div className="form-grid bootloader-form">
                <label>
                  Partition table (--ptable)
                  <select value={bootloader.ptable} onChange={setBootloaderField("ptable")}>
                    <option value="">(unset / default)</option>
                    {KNOWN_PTABLES.map((entry) => (
                      <option key={entry.value} value={entry.value}>
                        {entry.label}
                      </option>
                    ))}
                  </select>
                </label>
                <label>
                  Boot timeout (--timeout)
                  <input
                    type="number"
                    min="0"
                    value={bootloader.timeout}
                    placeholder="e.g. 5"
                    onChange={setBootloaderField("timeout")}
                  />
                </label>
                <label>
                  Install source (--source)
                  <input
                    list="bootloader-sources"
                    value={bootloader.source}
                    placeholder="e.g. bootimg-efi"
                    onChange={setBootloaderField("source")}
                  />
                  <datalist id="bootloader-sources">
                    {KNOWN_BOOTLOADER_SOURCES.map((entry) => (
                      <option key={entry} value={entry} />
                    ))}
                  </datalist>
                </label>
                <label>
                  Config file (--configfile)
                  <input
                    value={bootloader.configfile}
                    placeholder="optional path"
                    onChange={setBootloaderField("configfile")}
                  />
                </label>
                <label className="span-2">
                  Kernel append (--append)
                  <input
                    value={bootloader.append}
                    placeholder="console=ttyS0,115200 rootwait"
                    onChange={setBootloaderField("append")}
                  />
                </label>
                <label className="span-2">
                  Custom / extra options
                  <input
                    value={bootloader.extra}
                    placeholder="--any-custom-flag value"
                    onChange={setBootloaderField("extra")}
                  />
                </label>
              </div>
              <code className="bootloader-preview">{composeBootloaderLine(bootloader)}</code>
              <div className="form-actions">
                <button className="primary" onClick={applyBootloader} disabled={busy}>
                  Apply to WKS source
                </button>
                <button className="secondary" onClick={syncBootloader} disabled={busy}>
                  Read from editor
                </button>
              </div>

              <h2>WKS source</h2>
              {!layout.editable_in_place && (
                <label className="inline">
                  Save target layer
                  <select value={targetLayer} onChange={(event) => setTargetLayer(event.target.value)}>
                    {layout.project_layers.map((name) => (
                      <option key={name} value={name}>
                        {name}
                      </option>
                    ))}
                  </select>
                  <span className="muted small">
                    Inherited from a dependency layer; a project copy is saved and WKS_FILE is set.
                  </span>
                </label>
              )}
              <textarea
                className="wks-editor"
                value={draft}
                spellCheck={false}
                onChange={(event) => setDraft(event.target.value)}
              />
              <div className="form-actions">
                <button className="secondary" onClick={preview} disabled={busy}>
                  Preview parse
                </button>
              </div>
            </>
          )}
        </>
      )}
    </section>
  );
}
