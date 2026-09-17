import json
import os
import re
import sys
import tempfile
from pathlib import Path


def collect_layers(tinfoil):
    priorities = {}
    for entry in tinfoil.run_command("getLayerPriorities") or []:
        collection, _pattern, _regex, priority = entry
        priorities[collection] = priority
    layers = []
    for path in (tinfoil.config_data.getVar("BBLAYERS") or "").split():
        collection = None
        layer_conf = Path(path) / "conf" / "layer.conf"
        if layer_conf.is_file():
            match = re.search(r'BBFILE_COLLECTIONS\s*[+:]?=\s*"([^"]+)"', layer_conf.read_text())
            if match:
                collection = match.group(1).split()[0]
        name = collection or Path(path).name
        layers.append(
            {
                "name": name,
                "path": os.path.realpath(path),
                "priority": priorities.get(name),
            }
        )
    return layers


def collect_images(tinfoil):
    cache = tinfoil.cooker.recipecaches[""]
    image_files = {
        filename
        for filename, inherited in cache.inherits.items()
        if any(Path(class_file).name == "image.bbclass" for class_file in inherited)
    }
    images = []
    for name, filenames in sorted(cache.pkg_pn.items()):
        if not image_files.intersection(filenames):
            continue
        recipe_path = tinfoil.get_recipe_file(name)
        if recipe_path in image_files and not recipe_path.startswith("virtual:"):
            images.append({"name": name, "recipe_path": os.path.realpath(recipe_path)})
    return images


def collect_environment(tinfoil):
    data = tinfoil.config_data
    environment = {}
    for key in sorted(data.keys()):
        if key.startswith("__") or data.getVarFlag(key, "func", False):
            continue
        try:
            value = data.getVar(key)
        except Exception:
            continue
        if isinstance(value, str):
            environment[key] = value
    return environment


def _collect_conf_names(tinfoil, subdir):
    names = set()
    for path in (tinfoil.config_data.getVar("BBLAYERS") or "").split():
        directory = os.path.join(path, "conf", subdir)
        if os.path.isdir(directory):
            for entry in os.listdir(directory):
                if entry.endswith(".conf"):
                    names.add(entry[:-5])
    return sorted(names)


def collect_machines(tinfoil):
    return _collect_conf_names(tinfoil, "machine")


def collect_distros(tinfoil):
    return _collect_conf_names(tinfoil, "distro")


def collect_releases(tinfoil):
    names = tinfoil.config_data.getVar("LAYERSERIES_CORENAMES") or ""
    return sorted(set(names.split()))


def owning_layer(path, layers):
    if not path:
        return None
    real = os.path.realpath(path)
    best = None
    for layer in layers:
        base = layer["path"].rstrip("/") + "/"
        if real.startswith(base) and (best is None or len(layer["path"]) > len(best["path"])):
            best = layer
    return best["name"] if best else None


def collect_recipe_layers(tinfoil, layers):
    cache = tinfoil.cooker.recipecaches[""]
    mapping = {}
    for recipe_name, filenames in cache.pkg_pn.items():
        if not filenames:
            continue
        layer = owning_layer(sorted(filenames)[0], layers)
        if layer:
            mapping[recipe_name] = layer
    return mapping



def parse_layout(data, content, source_path):
    scripts_library = Path(data.getVar("COREBASE")) / "scripts" / "lib"
    sys.path.insert(0, str(scripts_library))
    import wic.ksparser

    wic.ksparser.get_bitbake_var = lambda name: data.getVar(name)
    expanded = content
    if source_path.endswith(".in"):
        expanded = data.expand(content)
        unresolved = re.compile(r"\$\{[^{}@\n\t :]+\}")
        while unresolved.search(expanded):
            expanded = unresolved.sub("", expanded)

    included_files = []

    class LayoutParser(wic.ksparser.KickStart):
        def _parse(self, parser, confpath):
            if included_files and str(confpath) in included_files:
                raise ValueError("Recursive or repeated WKS include: " + str(confpath))
            included_files.append(str(confpath))
            return super()._parse(parser, confpath)

    with tempfile.TemporaryDirectory(prefix="bitforge-wks-") as directory:
        preview_path = Path(directory) / "preview.wks"
        preview_path.write_text(expanded)
        layout = LayoutParser(str(preview_path))

    return {
        "partitions": [
            {
                "mountpoint": partition.mountpoint,
                "label": partition.label,
                "fstype": partition.fstype,
                "source": partition.source,
                "sourceparams": partition.sourceparams,
                "disk": partition.disk,
                "size_kib": partition.size,
                "fixed_size_kib": partition.fixed_size,
                "active": partition.active,
                "align": partition.align,
                "no_table": partition.no_table,
                "uuid": partition.uuid,
                "extra_space_kib": partition.extra_space,
                "overhead_factor": partition.overhead_factor,
            }
            for partition in layout.partitions
        ],
        "bootloader": vars(layout.bootloader),
        "includes": included_files[1:],
        "expanded_content": expanded,
    }


def image_layout(tinfoil, image, preview=None):
    import bb.data

    data = tinfoil.parse_recipe(image)
    if not bb.data.inherits_class("image", data):
        raise ValueError(image + " is not an image recipe")
    final_path = data.getVar("WKS_FULL_PATH") or ""
    template_path = data.getVar("WKS_TEMPLATE_PATH") or ""
    source_path = template_path or final_path
    recipe_path = data.getVar("FILE")
    result = {
        "image": image,
        "machine": data.getVar("MACHINE") or "",
        "recipe_path": recipe_path,
        "appends": list(tinfoil.get_file_appends(recipe_path)),
        "image_fstypes": (data.getVar("IMAGE_FSTYPES") or "").split(),
        "wic_enabled": bool(data.getVar("USING_WIC")),
        "wks_file": data.getVar("WKS_FILE") or "",
        "candidates": (data.getVar("WKS_FILES") or "").split(),
        "search_paths": (data.getVar("WKS_SEARCH_PATH") or "").split(":"),
        "final_path": os.path.realpath(final_path) if final_path else None,
        "source_path": os.path.realpath(source_path) if source_path else None,
        "template": bool(template_path) or source_path.endswith(".in"),
        "owning_layer": None,
        "content": "",
        "structure": None,
        "parse_error": None,
    }
    result["owning_layer"] = owning_layer(source_path, collect_layers(tinfoil))
    if not source_path:
        result["parse_error"] = "No WKS file resolves from WKS_FILES for this image and machine."
        return result
    if not Path(source_path).is_file():
        result["parse_error"] = "The resolved WKS source file does not exist: " + source_path
        return result
    if Path(source_path).stat().st_size > 1024 * 1024:
        raise ValueError("The WKS source exceeds the 1 MiB editor limit")
    content = Path(source_path).read_text() if preview is None else preview
    result["content"] = content
    try:
        result["structure"] = parse_layout(data, content, source_path)
    except Exception as error:
        result["parse_error"] = str(error)
    return result


def emit_event(**attrs):
    import xml.sax.saxutils as su

    body = " ".join("%s=%s" % (key, su.quoteattr(str(value))) for key, value in attrs.items())
    sys.stdout.write("BITFORGE_EVENT:<event %s/>\n" % body)
    sys.stdout.flush()


def recipe_of(taskfile):
    base = os.path.basename(taskfile or "")
    if base.endswith(".bb"):
        base = base[:-3]
    return base


def run_build(tinfoil, target):
    task_states = {
        "runQueueTaskStarted": "started",
        "runQueueTaskCompleted": "completed",
        "runQueueTaskFailed": "failed",
        "sceneQueueTaskStarted": "setscene-started",
        "sceneQueueTaskCompleted": "setscene-completed",
        "sceneQueueTaskFailed": "setscene-failed",
    }

    def callback(event):
        stats = getattr(event, "stats", None)
        if stats is not None:
            emit_event(
                kind="progress",
                total=getattr(stats, "total", 0),
                completed=getattr(stats, "completed", 0),
                active=getattr(stats, "active", 0),
                failed=getattr(stats, "failed", 0),
                skipped=getattr(stats, "skipped", 0),
                setscene_total=getattr(stats, "setscene_total", 0),
                setscene_active=getattr(stats, "setscene_active", 0),
                setscene_covered=getattr(stats, "setscene_covered", 0),
            )
        name = event.__class__.__name__
        if name in task_states:
            emit_event(
                kind="task",
                state=task_states[name],
                task=getattr(event, "taskname", ""),
                recipe=recipe_of(getattr(event, "taskfile", "")),
                taskhash=getattr(event, "taskhash", ""),
            )
        return False

    result = tinfoil.build_targets(
        target,
        task=None,
        handle_events=True,
        extra_events=[
            "bb.runqueue.runQueueTaskStarted",
            "bb.runqueue.runQueueTaskCompleted",
            "bb.runqueue.runQueueTaskFailed",
            "bb.runqueue.sceneQueueTaskStarted",
            "bb.runqueue.sceneQueueTaskCompleted",
            "bb.runqueue.sceneQueueTaskFailed",
        ],
        event_callback=callback,
    )
    emit_event(kind="build", state="succeeded" if result else "failed")
    return bool(result)


def run_plan(tinfoil, target):
    tinfoil.run_command("setConfig", "dry_run", True)
    dirty = set()
    clean = set()

    def callback(event):
        name = event.__class__.__name__
        if name == "runQueueTaskStarted" and not getattr(event, "noexec", False):
            dirty.add(recipe_of(getattr(event, "taskfile", "")))
        elif name == "sceneQueueTaskCompleted":
            clean.add(recipe_of(getattr(event, "taskfile", "")))
        return False

    tinfoil.build_targets(
        target,
        task=None,
        handle_events=True,
        extra_events=["bb.runqueue.sceneQueueTaskCompleted"],
        event_callback=callback,
    )
    dirty.discard("")
    clean.discard("")
    return {
        "image": target,
        "dirty": sorted(dirty),
        "clean": sorted(clean - dirty),
    }


def main():
    root = Path(sys.argv[1]).resolve()
    sys.path.insert(0, str(root / "ForgeSource" / "bitbake" / "lib"))
    import bb.tinfoil

    action = sys.argv[2]
    with bb.tinfoil.Tinfoil() as tinfoil:
        tinfoil.prepare(config_only=(action == "environment"))
        if action == "build":
            ok = run_build(tinfoil, sys.argv[3])
            print("BITFORGE_METADATA:" + json.dumps({"result": ok}))
            if not ok:
                sys.exit(1)
            return
        if action == "plan":
            result = run_plan(tinfoil, sys.argv[3])
            print("BITFORGE_METADATA:" + json.dumps(result))
            return
        if action == "metadata":
            result = {
                "images": collect_images(tinfoil),
                "layers": collect_layers(tinfoil),
                "machines": collect_machines(tinfoil),
                "distros": collect_distros(tinfoil),
                "releases": collect_releases(tinfoil),
                "environment": collect_environment(tinfoil),
                "multiconfig": (tinfoil.config_data.getVar("BBMULTICONFIG") or "").split(),
            }
            result["recipe_layers"] = collect_recipe_layers(tinfoil, result["layers"])
        elif action == "environment":
            result = {"environment": collect_environment(tinfoil)}
        elif action in ("layout", "preview"):
            preview = json.load(sys.stdin)["content"] if action == "preview" else None
            result = image_layout(tinfoil, sys.argv[3], preview)
        else:
            raise ValueError("Unknown metadata action: " + action)
        result["bblayers"] = (tinfoil.config_data.getVar("BBLAYERS") or "").split()
        print("BITFORGE_METADATA:" + json.dumps(result))


if __name__ == "__main__":
    try:
        main()
    except Exception as error:
        print("BITFORGE_METADATA:" + json.dumps({"error": str(error)}))
        sys.exit(1)