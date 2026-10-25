"""Refuse loose import shadows, bytecode substitutions and external model files."""
from __future__ import annotations

import importlib.machinery
import ast
import json
from pathlib import Path
import sys
import sysconfig
import zipimport

import capture_launch
import gliner_identity
from capture_identity import digest


def import_inventory(files, roots, *, path=None, hooks=None):
    """Installed membership comes from RECORD, never from selected module origins."""
    files = {Path(name).absolute(): sha for name, sha in files.items()}
    roots = {Path(root).absolute() for root in roots}
    here = Path(__file__).resolve().parent
    standard_library = Path(sysconfig.get_path("stdlib"))
    approved = roots | {standard_library, Path(sysconfig.get_path("platstdlib")), standard_library / "lib-dynload",
                        here, here.parent, here / "tuned_presidio", here / "theirbench"}
    for entry in sys.path if path is None else path:
        selected = Path(entry or ".").absolute()
        if selected.exists() and selected not in approved:
            raise ValueError("external Python import root is unsupported")
    if any(name in sys.modules for name in ("sitecustomize", "usercustomize")):
        raise ValueError("customized Python startup is unsupported")
    for hook in sys.path_hooks:
        if (getattr(hook, "__module__", None), getattr(hook, "__name__", None)) not in {
                ("zipimport", "zipimporter"), ("_frozen_importlib_external", "path_hook_for_FileFinder")}:
            raise ValueError("unreviewed Python path hook is unsupported")
    for finder in sys.path_importer_cache.values():
        if finder is not None and not isinstance(finder, (importlib.machinery.FileFinder, zipimport.zipimporter)):
            raise ValueError("unreviewed cached importer is unsupported")
    bootstraps = {}
    bootstrap_pths = set()
    standard = {"_frozen_importlib.BuiltinImporter", "_frozen_importlib.FrozenImporter",
                "_frozen_importlib_external.PathFinder"}
    for hook in sys.meta_path if hooks is None else hooks:
        name = ((hook.__module__ + "." + hook.__qualname__) if isinstance(hook, type)
                else type(hook).__module__ + "." + type(hook).__qualname__)
        if name not in standard:
            proof = gliner_identity._bootstrap_context(hook)
            bootstraps[Path(proof["path"]).absolute()] = proof["sha256"]
            bootstrap_pths.add(Path(proof["pth"]).absolute())
    capture_launch.check_harness()
    modules = set()
    for root in roots:
        for file in root.rglob("*"):
            if file.is_symlink() and file.is_dir():
                raise ValueError("linked import directory is unsupported")
            native = any(file.name.endswith(suffix) for suffix in importlib.machinery.EXTENSION_SUFFIXES)
            if file.suffix == ".py" or native:
                if file.absolute() not in files and file.absolute() not in bootstraps:
                    raise ValueError("import source is absent from installed content inventory")
                if file.suffix == ".py":
                    gliner_identity._check_bytecode(file)
            elif file.suffix == ".pyc" and file.parent.name != "__pycache__":
                raise ValueError("sourceless import bytecode is unsupported")
            elif file.suffix == ".pth" and file.absolute() not in bootstrap_pths:
                if any(line.strip() and not line.lstrip().startswith("#") for line in file.read_text().splitlines()):
                    raise ValueError("unreviewed Python startup file is unsupported")
    directories = {parent for file in files for parent in file.parents}
    for file in files:
        for root in roots:
            if file.is_relative_to(root):
                relative = file.relative_to(root)
                name = relative.parts[0] if len(relative.parts) > 1 else file.name.partition(".")[0]
                native = any(file.name.endswith(suffix) for suffix in importlib.machinery.EXTENSION_SUFFIXES)
                if name.isidentifier() and (len(relative.parts) > 1 or file.suffix == ".py" or native):
                    modules.add(name)
    origins = {}
    for name in sorted(modules):
        spec = importlib.machinery.PathFinder._get_spec(name, list(sys.path if path is None else path))
        if spec is None:
            raise ValueError("installed import origin is unavailable")
        if spec.origin is None:
            locations = [Path(value).absolute() for value in spec.submodule_search_locations or ()]
            if not locations or any(location not in directories for location in locations):
                raise ValueError("namespace import shadow is unsupported")
            origins[name] = sorted(map(str, locations))
        else:
            origin = Path(spec.origin).absolute()
            if (not isinstance(spec.loader, (importlib.machinery.SourceFileLoader, importlib.machinery.ExtensionFileLoader))
                    or origin not in files):
                raise ValueError("selected import differs from installed content inventory")
            origins[name] = str(origin)
    return digest({"origins": origins, "bootstraps": {str(path): sha for path, sha in bootstraps.items()}})


def model_references(root, file):
    """File selectors must resolve to bytes already inside the selected tree."""
    def check(value):
        if isinstance(value, dict):
            for key, item in value.items():
                selector = (key in {"file", "files", "path", "paths"}
                            or key.endswith(("_file", "_files", "_path", "_paths"))) and key != "_name_or_path"
                if selector and item is not None:
                    references = item if isinstance(item, list) else [item]
                    for reference in references:
                        if not isinstance(reference, str):
                            raise ValueError("unsupported model file selector")
                        path = Path(reference)
                        if (path.is_absolute() or ".." in path.parts or reference.startswith("~")
                                or ":" in reference or not (root / path).exists()):
                            raise ValueError("model file selector escapes selected input tree")
                check(item)
        elif isinstance(value, list):
            for item in value:
                check(item)
    if file.suffix == ".json":
        check(json.loads(file.read_text(encoding="utf-8")))
    elif file.suffix == ".cfg":
        for line in file.read_text(encoding="utf-8").splitlines():
            # spaCy's config permits path variables and package references. Absolute
            # and parent-relative selectors can read bytes outside the saved model.
            _, separator, value = line.partition("=")
            value = value.strip()
            if separator and value.startswith(('"', "'")):
                literal = ast.literal_eval(value)
                if Path(literal).is_absolute() or ".." in Path(literal).parts or literal.startswith("~"):
                    raise ValueError("external model configuration selector is unsupported")
