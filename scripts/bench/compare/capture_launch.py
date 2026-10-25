"""Establish source-owned import slots once before planning private captures."""
from pathlib import Path
import sys

_initialized = False


def initialize():
    global _initialized
    if _initialized:
        return
    here = Path(__file__).resolve().parent
    # Preserve every pre-existing entry, including duplicate external entries.
    sys.path[:] = [str(here.parent), str(here), str(here / "tuned_presidio"),
                   str(here / "theirbench"), *sys.path]
    _initialized = True
