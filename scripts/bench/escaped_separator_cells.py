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
    cuts = api.Rng(api.PARTITION_SEEDS[partition], "escaped-separator-interior")
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
        host = str(rng.between(100, 254))
        ip = ("10.", "74." if dev else "83.", str(rng.between(1, 254)) + ".", host)
        example = ("192.", "0.", "2.", host) if dev else ("198.", "51.", "100.", host)
        # Areas 900–999 cannot be issued. A public stock code uses the same digits.
        ssn = (str(rng.between(900, 949) if dev else rng.between(950, 999)) + "-",
               rng.digits(2) + "-", rng.digits(4))
        # The all-zero German bank routing code is unassignable, even with a valid mod-97.
        iban = api._by_four(api._iban("DE", "00000000" + ("3" if dev else "8") + rng.digits(9)))
        phone = ("+1 ", "(312) " if dev else "(415) ", "555-", "01" + rng.digits(2))  # NANPA 555-01xx
        pairs = (
            # (group, cut) is shared by the personal value and its benign twin.
            ("ip", "IPADDRESS", "Device IP", ip, "Documentation address", example, 3, cuts.between(1, 2)),
            ("ssn", "SSN", "SSN", ssn, "Public stock code", ssn, 1, 1),
            ("iban", "IBAN", "IBAN", iban, "Catalog reference", ("ZZ" + iban[0][2:], *iban[1:]),
             cuts.between(3, 4), cuts.between(1, 3)),
            ("phone", "TELEPHONENUM", "Telephone", phone, "Dial plan placeholder", ("+0 ", "(000) ", *phone[2:]),
             2, cuts.between(1, 2)),
        )
        views = (str(rng.between(10, 49) if dev else rng.between(50, 99)), rng.digits(3), rng.digits(3))
        emit("D", "views", "literal_zwsp_views", index,
             "The clip reached {V} views." if dev else "The public counter shows {V} views so far.",
             "", "\u200b".join(views), "decoy:count")
        for cp, name in CONTROLS:
            for kind, syntax, sep in (
                ("json", "unicode", rf"\u{cp:04x}" if dev else rf"\u{cp:04X}"),
                ("html", "decimal", f"&#{cp};"), ("html", "hex", f"&#x{cp:X};"),
                ("html", "named", f"&{name};"),
            ):
                surface = f"{kind}_{syntax}_{cp:04x}"
                single, repeated = TEMPLATES[partition][kind]
                for family, label, cue, groups, public_cue, twin, group, cut in pairs:
                    def inside(parts):
                        at = sum(map(len, parts[:group])) + cut
                        plain = "".join(parts)
                        return plain[:at] + sep + plain[at:]

                    for name_suffix, value, public in (("", sep.join(groups), sep.join(twin)),
                                                       ("_inside", inside(groups), inside(twin))):
                        name_surface = surface + name_suffix
                        emit("A", family, name_surface, index, single, cue, value, label)
                        emit("R", family, name_surface, index, repeated, cue, value, label)
                        emit("D", family + "_twin", name_surface, index, single, public_cue, public, "decoy:" + family)
                emit("D", "views", surface, index, single, "Page views" if dev else "Video views",
                     sep.join(views), "decoy:count")
                if index == 0:
                    words = ("co", "operate") if dev else ("inter", "change")
                    emit("D", "word", surface, index, single, "Typography sample", sep.join(words), "decoy:word")
                    # A manual displaying escape syntax, not a second encoding layer to unwrap.
                    literal = "\\" + sep if kind == "json" else "&amp;" + sep[1:]
                    emit("D", "notation", surface, index, single, "Escape spelling", literal, "decoy:notation")

    # Hold the code and its cue constant across carriers. These D examples
    # are public schema/markup fixtures, not intentional disclosures. A second
    # decoding pass or a scan outside visible HTML text would invent one.
    carrier_rng = api.Rng(api.PARTITION_SEEDS[partition], "escaped-separator-carriers")
    key = "note" if dev else "body"
    for index in range(2):
        area = carrier_rng.between(900, 949) if dev else carrier_rng.between(950, 999)
        mid, tail = carrier_rng.digits(2), carrier_rng.digits(4)
        def ssn(separator):
            return f"{area}-{mid[0]}{separator}{mid[1]}-{tail}"

        for surface, template, raw in (
            ("json_double", f'{{"{key}":"Sample SSN: {{V}}"}}', ssn(r"\\u200b")),
            ("html_double", "<p>Sample SSN: {V}</p>", ssn("&amp;#8203;")),
            ("json_key", f'{{"schema":{{"SSN: {{V}}":"{key}"}}}}', ssn(r"\u200b")),
            ("html_attribute", '<p data-sample="SSN: {V}">Preview</p>', ssn("&#8203;")),
            ("html_comment", "<!-- SSN: {V} --><p>Preview</p>", ssn("&#8203;")),
            ("html_script", '<script>const sample = "SSN: {V}";</script>', ssn("&#8203;")),
        ):
            emit("D", "carrier_ssn", "carrier_" + surface, index, template, "", raw, "decoy:ssn")

        phone = ("+1 (312)" if dev else "+1 (415)") + r"\t5\u200b55-01" + carrier_rng.digits(2)
        for surface, family, label, opening, closing, raw in (
            ("json_before_cue", "ssn", "SSN", f'{{"{key}":"Summary' + r'\nSSN: ', '"', ssn(r"\u200b")),
            ("json_phone_space", "phone", "TELEPHONENUM", f'{{"{key}":"Telephone: ', '"', phone),
            ("html_doctype", "ssn", "SSN", "<!DOCTYPE html>SSN: ", "", ssn("&#8203;")),
            ("html_zwj", "ssn", "SSN", "<p>SSN: ", "</p>", ssn("&zwj;")),
            ("json_bom", "ssn", "SSN", f'{{"{key}":"SSN: ', '"', ssn(r"\uFEFF")),
        ):
            text = opening + "{V}" + closing
            is_json = surface.startswith("json_")
            emit("A", family, "carrier_" + surface, index, text + ("}" if is_json else ""), "", raw, label)
            repeat = ',"copy":"{V}"}' if is_json else "<p>{V}</p>"
            emit("R", family, "carrier_" + surface, index, text + repeat, "", raw, label)
    return result
