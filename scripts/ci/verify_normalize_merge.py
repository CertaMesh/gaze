#!/usr/bin/env python3
"""Compare shared normalization against both immutable parents of its relocation merge."""
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[2]
PARENTS = {
    "branch": ("a0316b08782361d187a250f4496147e543d3835d", "crates/gaze/src/normalize.rs"),
    "main": ("cd54f1a8d388a02c2fc0ed0b0f92dd084d9cbd6f", "crates/gaze-types/src/normalize.rs"),
}


def main():
    target = ROOT / "crates/gaze-types/tests/normalize_merge_parents.rs"
    if target.exists():
        raise SystemExit("refusing to overwrite an existing test")
    modules = []
    for name, (sha, path) in PARENTS.items():
        source = subprocess.check_output(["git", "show", f"{sha}:{path}"], cwd=ROOT, text=True)
        modules.append(f"#[allow(dead_code)] mod {name} {{\n{source}\n}}\n")
    test = r'''
use gaze_types::normalize as current;
#[test]
fn both_parent_normalizers_preserve_all_scalar_and_source_mappings() {
    let ignorables = regex::Regex::new(r"\p{Default_Ignorable_Code_Point}").unwrap();
    let mut compared_main = 0;
    let mut compared_branch = 0;
    for ch in (0..=0x10ffff).filter_map(char::from_u32) {
        let raw = ch.to_string();
        let actual = current::normalize(&raw);
        let previous = branch::normalize(&raw);
        assert_eq!((&actual.text, &actual.spans), (&previous.text, &previous.spans), "branch U+{:04X}", ch as u32);
        compared_branch += 1;
        // The PR deliberately adds ignorable removal and the U+2011 hyphen fold.
        if ch != '\u{2011}' && !ignorables.is_match(&raw) {
            let previous = main::normalize(&raw);
            assert_eq!((&actual.text, &actual.spans), (&previous.text, &previous.spans), "main U+{:04X}", ch as u32);
            compared_main += 1;
        }
    }
    for raw in ["e\u{301} Ａ\u{200b}Ｂ\u{2011}Ｃ", "\r\n\u{feff}क्\u{200d}ष\u{34f}終", "A\u{344}Z"] {
        let actual = current::normalize(raw);
        let previous = branch::normalize(raw);
        assert_eq!((&actual.text, &actual.spans), (&previous.text, &previous.spans));
        for start in 0..actual.spans.len() {
            assert_eq!(current::raw_range(start..actual.spans.len(), &actual.spans), branch::raw_range(start..previous.spans.len(), &previous.spans));
        }
    }
    println!("NORMALIZE MERGE: {compared_branch} branch scalars; {compared_main} unchanged main scalars; exact text and provenance");
}
'''
    target.parent.mkdir(exist_ok=True)
    try:
        target.write_text("\n".join(modules) + test)
        return subprocess.run(["nice", "-n", "10", "cargo", "test", "-p", "gaze-types", "--test", "normalize_merge_parents", "--", "--nocapture"], cwd=ROOT).returncode
    finally:
        target.unlink(missing_ok=True)


if __name__ == "__main__":
    raise SystemExit(main())
