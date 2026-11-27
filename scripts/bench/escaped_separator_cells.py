"""CC0 wire-spelling cells. Gold is the inserted source, never its decoded form."""

SURFACE_PREFIX = "escaped_separator_"
# HTML5 names, including their required semicolon; numeric references use the
# same code points. Do not emit the literal glyphs as additional positives.
CONTROLS = ((0x200B, "ZeroWidthSpace"), (0x2060, "NoBreak"), (0x00AD, "shy"), (0x200C, "zwnj"))

TEMPLATES = {
    "dev": {
        "json": ('{"note":"Résumé — {C}: {V}"}', '{"note":"Résumé — {C}: {V}","copy":"{V}"}'),
        "html": ("<p>Résumé — {C}: {V}</p>", "<p>Résumé — {C}: {V}</p><output>{V}</output>"),
    },
    "test": {
        "json": ('{"result":{"message":"Überblick: {C} = {V}"}}',
                 '{"result":{"message":"Überblick: {C} = {V}","again":["{V}"]}}'),
        "html": ("<li>Überblick: <span>{C} = {V}</span></li>",
                 "<li>Überblick: <span>{C} = {V}</span></li><li>{V}</li>"),
    },
}


def records(api, partition):
    rng = api.Rng(api.PARTITION_SEEDS[partition], "escaped-separator-source")
    dev = partition == "dev"
    result = []

    def emit(layer, family, surface, index, template, cue, raw, label):
        text, gold, decoys = api._fill_with_decoys(template, {"C": (cue, None), "V": (raw, label)})
        result.append(api.Record(
            uid=f"agentic-{partition}-{layer}-escsep-{family}-{surface}-{index}", partition=partition,
            layer=layer, family=family, surface=SURFACE_PREFIX + surface,
            group=f"{partition}-escsep-{family}-{index}", template=f"escsep/{partition}/{layer}/{surface}",
            language="en", region="US", validity=api.BENIGN if layer == "D" else api.UNCHECKED,
            text=text, gold=gold, decoys=decoys))

    for index in range(api.DOCS_PER_FAMILY):
        # A private device on RFC 1918 space vs a documentation example on TEST-NET.
        host = str(rng.between(1, 254))
        ip = ("10.", "74." if dev else "83.", str(rng.between(1, 254)) + ".", host)
        example = ("192.", "0.", "2.", host) if dev else ("198.", "51.", "100.", host)
        # Areas 900–999 cannot be issued. A public stock code uses the same digits.
        ssn = (str(rng.between(900, 949) if dev else rng.between(950, 999)) + "-",
               rng.digits(2) + "-", rng.digits(4))
        # The all-zero German bank routing code is unassignable, even with a valid mod-97.
        iban = api._by_four(api._iban("DE", "00000000" + ("3" if dev else "8") + rng.digits(9)))
        phone = ("+1", "312" if dev else "415", "555", "01" + rng.digits(2))  # NANPA 555-01xx
        pairs = (
            ("ip", "IPADDRESS", "Device IP", ip, "Documentation address", example),
            ("ssn", "SSN", "SSN", ssn, "Public stock code", ssn),
            ("iban", "IBAN", "IBAN", iban, "Catalog reference", ("ZZ" + iban[0][2:], *iban[1:])),
            ("phone", "TELEPHONENUM", "Telephone", phone, "Dial plan placeholder", ("+0", *phone[1:])),
        )
        views = (str(rng.between(1, 4) if dev else rng.between(5, 9)), rng.digits(3), rng.digits(3))
        for cp, name in CONTROLS:
            for kind, syntax, sep in (
                ("json", "unicode", rf"\u{cp:04x}" if dev else rf"\u{cp:04X}"),
                ("html", "decimal", f"&#{cp};"), ("html", "hex", f"&#x{cp:X};"),
                ("html", "named", f"&{name};"),
            ):
                surface = f"{kind}_{syntax}_{cp:04x}"
                single, repeated = TEMPLATES[partition][kind]
                for family, label, cue, groups, public_cue, twin in pairs:
                    value = sep.join(groups)
                    emit("A", family, surface, index, single, cue, value, label)
                    emit("R", family, surface, index, repeated, cue, value, label)
                    emit("D", family + "_twin", surface, index, single, public_cue, sep.join(twin), "decoy:" + family)
                emit("D", "views", surface, index, single, "Page views" if dev else "Video views",
                     sep.join(views), "decoy:count")
                if index == 0:
                    words = ("co", "operate") if dev else ("inter", "change")
                    emit("D", "word", surface, index, single, "Typography sample", sep.join(words), "decoy:word")
                    # A manual displaying escape syntax, not a second encoding layer to unwrap.
                    literal = "\\" + sep if kind == "json" else "&amp;" + sep[1:]
                    emit("D", "notation", surface, index, single, "Escape spelling", literal, "decoy:notation")
    return result
