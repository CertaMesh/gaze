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

    wrappers = (["({V})", '{"output":"{V}"}', "word{V}", "令牌{V}已隐藏", r"Bearer%20{V}"]
                if partition == "dev" else
                ["[{V}]", '{"result":{"value":"{V}"}}', "BUILD_{V}", "输出{V}结束", r"auth%3D{V}"])
    breaks = {"lf": "\n", "crlf": "\r\n", "json": r"\n", "nested_json": r"\\n",
              "unicode": r"\u000a", "html": "<br>", "shell": "\\\n",
              "concat": "\\n' +\n'", "comment": "\n# ", "slash": "\n// ",
              "quote": "\n> ", "star": "\n* "}
    for index in range(api.DOCS_PER_FAMILY):
        for prefix, width in [("ghp_", 36), ("gho_", 36), ("ghu_", 36), ("ghr_", 36),
                              ("ghs_", 36), ("github_pat_", 82), ("sk-proj-", 48),
                              ("sk-svcacct-", 80), ("sk-admin-", 48), ("sk-None-", 48),
                              ("sk-ant-api03-", 60), ("xoxb-", 60), ("xapp-", 96),
                              ("xoxe-", 140), ("xoxe.xoxp-", 140), ("sk_live_", 32),
                              ("rk_test_", 32), ("whsec_", 32), ("AIza", 35)]:
            value = prefix + chars(width)
            for surface, template in enumerate(wrappers):
                emit("A", prefix.rstrip("_-"), "bare_" + str(surface), index, value, template)
            emit("R", prefix.rstrip("_-"), "repeat", index, value, wrappers[0] + " " + wrappers[1])
            # Missing attribution and near-prefix words never become credentials.
            emit("D", prefix.rstrip("_-"), "hash", index, chars(width), wrappers[1])
        for prefix in ["ghp_", "sk-proj-"]:
            value = prefix + chars(300) + "-old"
            emit("A", prefix.rstrip("_-"), "overlong", index, value, wrappers[1])
        generic = "sk-" + chars(48)
        emit("A", "generic_sk", "boundary", index, generic, wrappers[0])
        for prefix in ["task-", "desk-", "mask-"]:
            emit("D", "generic_sk", prefix + "identifier", index, prefix + chars(64), wrappers[1])
        for label in ["PRIVATE KEY", "RSA PRIVATE KEY", "EC PRIVATE KEY", "DSA PRIVATE KEY",
                      "OPENSSH PRIVATE KEY", "ENCRYPTED PRIVATE KEY", "PGP PRIVATE KEY BLOCK"]:
            body = chars(64)
            family = label.lower().replace(" ", "_")
            for surface, br in breaks.items():
                value = f"-----BEGIN {label}-----{br}{body}{br}-----END {label}-----"
                emit("A", family, "pem_" + surface, index, value, wrappers[1])
                truncated = f"-----BEGIN {label}-----{br}{body}"
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
