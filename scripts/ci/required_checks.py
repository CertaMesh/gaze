"""Validate required job results in both full and Markdown-only CI."""

import json
import os


OWNERS = {
    "test": {"test-support", "workspace-shards", "test-coverage"},
    "xtask": {"xtask-feature-gates", "workspace-default"},
}


def require_success(check: str, needs: dict) -> None:
    changes = needs["changes"]
    if changes["result"] != "success":
        raise ValueError("change classification and its tests must succeed")
    docs = changes.get("outputs", {}).get("docs_only") == "true"
    expected = {owner: "skipped" if docs else "success" for owner in OWNERS[check]}
    expected["changes"] = "success"
    if check == "xtask":
        expected["test"] = "success"
        expected["markdown-gates"] = "success" if docs else "skipped"
    if set(needs) != set(expected):
        raise ValueError("missing or unexpected required CI owners")
    for owner, result in expected.items():
        if needs[owner]["result"] != result:
            raise ValueError(f"{owner} must be {result}, got {needs[owner]['result']}")


if __name__ == "__main__":
    require_success(os.environ["REQUIRED_CHECK"], json.loads(os.environ["NEEDS"]))
