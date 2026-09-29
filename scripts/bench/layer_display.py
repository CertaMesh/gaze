"""Human-readable names for the scored Gaze comparison corpora."""

LAYER_DISPLAY_NAMES = {
    "C": "Kiji EN/DE holdout and A4 negatives",
    "A": "Synthetic identifiers in agentic formats",
    "D": "Synthetic benign lookalikes",
    "R": "Repeated PII values with decoys",
}


def layer_display_name(key: str) -> str:
    return LAYER_DISPLAY_NAMES[key]
