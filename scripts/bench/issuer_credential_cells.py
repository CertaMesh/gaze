"""CC0 issuer credentials and private armour; source gold is minted independently.

All suffixes are synthetic and cannot authenticate. Public armour and ordinary
code/hash identifiers are counterweights, never private material.
"""


def records(api, partition):
    rng = api.Rng(api.PARTITION_SEEDS[partition], "issuer-credentials-v29")
    result = []

    def emit(layer, family, surface, index, value, template):
        text, gold, decoys = api._fill_with_decoys(
            template, {"V": (value, None if layer == "D" else "SECURITYTOKEN")})
        result.append(api.Record(
            uid=f"agentic-{partition}-{layer}-issuer-{family}-{surface}-{index}",
            partition=partition, layer=layer, family=family,
            surface="issuer_credentials_" + surface,
            validity=api.BENIGN if layer == "D" else api.UNCHECKED,
            group=f"{partition}-issuer-{family}-{index}",
            template=f"issuer/{partition}/{surface}", language="en", region="US",
            text=text, gold=gold, decoys=decoys))

    def chars(size, alphabet="ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789"):
        return "".join(alphabet[rng.below(len(alphabet))] for _ in range(size))

    wrappers = (["({V})", '{"output":"{V}"}', "GH_TOKEN_{V}", "令牌{V}已隐藏", r"Bearer%20{V}"]
                if partition == "dev" else
                ["[{V}]", '{"result":{"value":"{V}"}}', "BUILD_{V}", "输出{V}结束", r"auth%3D{V}"])
    breaks = {"lf": "\n", "crlf": "\r\n", "json": r"\n", "nested_json": r"\\n",
              "unicode": r"\u000a", "html": "<br>", "shell": "\\\n",
              "concat": "\\n' +\n'", "comment": "\n# ", "slash": "\n// ",
              "quote": "\n> ", "star": "\n* ", "space": " ", "tab": "\t",
              "comma": '\\n", "', "comma_lines": '\\n",\n    "', "url": "%0A"}
    for index in range(api.DOCS_PER_FAMILY):
        for prefix, width in [("ghp_", 36), ("gho_", 36), ("ghu_", 36), ("ghr_", 36),
                              ("ghs_", 36), ("github_pat_", 82), ("sk-proj-", 48),
                              ("sk-svcacct-", 80), ("sk-admin-", 48), ("sk-None-", 48),
                              ("sk-ant-api03-", 60), ("xoxb-", 60), ("xapp-", 96),
                              ("xoxe-", 140), ("xoxe.xoxp-", 140), ("sk_live_", 32),
                              ("rk_test_", 32), ("whsec_", 32), ("AIzaSy", 33)]:
            value = prefix + chars(width)
            for surface, template in enumerate(wrappers):
                emit("A", prefix.rstrip("_-"), "bare_" + str(surface), index, value, template)
            emit("R", prefix.rstrip("_-"), "repeat", index, value, wrappers[0] + " " + wrappers[1])
            # Missing attribution and near-prefix words never become credentials.
            emit("D", prefix.rstrip("_-"), "hash", index, chars(width), wrappers[1])
            if prefix == "xoxe.xoxp-":
                emit("A", "xoxp", "embedded_refresh", index, value[len("xoxe."):],
                     wrappers[1].replace("{V}", "wordxoxe.{V}"))
            else:
                emit("D", prefix.rstrip("_-"), "lowercase_identifier", index,
                     "word" + value, wrappers[1])
        for prefix in ["ghp_", "sk-proj-"]:
            value = prefix + chars(300) + "-old"
            emit("A", prefix.rstrip("_-"), "overlong", index, value, wrappers[1])
        generic = "sk-" + chars(48)
        emit("A", "generic_sk", "boundary", index, generic, wrappers[0])
        for prefix in ["task-", "desk-", "mask-"]:
            emit("D", "generic_sk", prefix + "identifier", index, prefix + chars(64), wrappers[1])
        identifiers = [
            "network_test_handles_missing_bundle_gracefully",
            "benchmark_test_reports_peak_rss_for_every_arm",
            "task_test_retry_with_exponential_backoff_ok",
            "parser_weighs_all_candidate_spans_before_conflict_resolution",
            "user_laughs_at_the_joke_and_continues_reading_page_two",
            "disk_live_bytes_written_since_the_last_collection",
            "desk-admin-panel-settings-overview-page-container",
            "task-proj-board-column-header-collapsed-state",
            "mask-None-values-in-the-output-stream",
        ]
        for number, identifier in enumerate(identifiers):
            emit("D", "code_identifier", str(number), index,
                 identifier + "_" + chars(12), wrappers[1])
        emit("D", "tutorial", "multiline", index,
             "-----BEGIN OPENSSH PRIVATE KEY-----\nand then contains base64 lines you must never share with anyone at all. " + chars(8),
             "Your file starts with {V}" if partition == "dev" else "Instructions: {V}")
        emit("D", "tutorial", "single_line", index,
             "-----BEGIN PRIVATE KEY----- and ends with a matching footer. " + chars(8),
             "A file starts with {V}" if partition == "dev" else "A key begins with {V}")
        emit("D", "google_near_prefix", "base64", index, "BAIza" + chars(40), wrappers[1])
        for label in ["PRIVATE KEY", "RSA PRIVATE KEY", "EC PRIVATE KEY", "DSA PRIVATE KEY",
                      "OPENSSH PRIVATE KEY", "ENCRYPTED PRIVATE KEY", "PGP PRIVATE KEY BLOCK"]:
            body = chars(64)
            family = label.lower().replace(" ", "_")
            for surface, br in breaks.items():
                framed_label = label.replace(" ", "%20") if surface == "url" else label
                gap = "%20" if surface == "url" else " "
                value = f"-----BEGIN{gap}{framed_label}-----{br}{body}{br}-----END{gap}{framed_label}-----"
                emit("A", family, "pem_" + surface, index, value, wrappers[1])
                truncated = f"-----BEGIN{gap}{framed_label}-----{br}{body}"
                emit("A", family, "truncated_" + surface, index, truncated, wrappers[1])
            emit("A", family, "mismatched", index,
                 f"-----BEGIN {label}-----\n{body}\n-----END PRIVATE KEY-----", wrappers[1])
        for label in ["PUBLIC KEY", "CERTIFICATE", "PGP PUBLIC KEY BLOCK"]:
            emit("D", "public_armour", label.lower().replace(" ", "_"), index,
                 f"-----BEGIN {label}-----\n{chars(64)}\n-----END {label}-----", wrappers[1])
        for surface, placeholder in [("dots", "..."), ("instructions", "<your key here>")]:
            emit("D", "placeholder", surface, index,
                 f"-----BEGIN PRIVATE KEY-----\n{placeholder}\n-----END PRIVATE KEY-----", wrappers[1])
    return result
