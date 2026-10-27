"""CC0 source-authored Unicode identifier and benign formatting cells.

Values are seeded independently of the runtime regression fixtures. Gold is
the entire producer value, including its original UTF-8 formatting characters.
No detector output determines gold or the negative examples.
"""

SEPARATORS = {"zwsp": "\u200b", "word_joiner": "\u2060", "soft_hyphen": "\u00ad", "nb_hyphen": "\u2011", "bom": "\ufeff"}
CUED_SURFACES = {"invisible_" + separator + "_" + surface
                 for separator in SEPARATORS for surface in ("prose", "json")}


def records(api, partition):
    result = []
    rng = api.Rng(api.PARTITION_SEEDS[partition], "invisible-separators")
    cues = {"dev": ("Tax ID", "IBAN", "Phone", "Card", "Email"),
            "test": ("Tax number", "IBAN number", "Telephone", "Payment card", "Mail")}[partition]
    templates = {"dev": {"prose": "Please use {C}: {V}; approved.", "json": '{"{C}":"{V}","ok":true}'},
                 "test": {"prose": "Account details — {C} = {V}. End.", "json": '{"result":{"{C}":"{V}"}}'}}[partition]

    def emit(layer, family, surface, index, template, value, label, cue=""):
        text, gold, decoys = api._fill_with_decoys(
            template, {"V": (value, label if layer != "D" else None), "C": (cue, None)})
        result.append(api.Record(
            uid=f"agentic-{partition}-{layer}-invisible-{family}-{surface}-{index}",
            partition=partition, layer=layer, family=family, surface="invisible_" + surface,
            validity=api.BENIGN if layer == "D" else api.UNCHECKED,
            group=f"{partition}-invisible-{family}-{index}", template=f"invisible/{partition}/{surface}",
            language="en", region="US", text=text, gold=gold, decoys=decoys))

    for index in range(api.DOCS_PER_FAMILY):
        # Reserved NANPA fiction range; different area codes in each partition.
        phone = ("+1", "202" if partition == "dev" else "212", "555", f"01{rng.below(100):02d}")
        tax = str(rng.between(1, 9)) + rng.digits(9)
        email = ("cedar" if partition == "dev" else "willow") + rng.digits(5)
        values = (
            ("TAXNUM", (tax[:3], tax[3:6], tax[6:])),
            ("IBAN", api._by_four(api._iban_gb(rng) if partition == "dev" else api._iban_nl(rng))),
            ("TELEPHONENUM", phone),
            ("CREDITCARDNUMBER", api._by_four(api._card(rng))),
            ("EMAIL", (email[:3], email[3:] + "@example.invalid")),
        )
        for separator_name, separator in SEPARATORS.items():
            for cue, (label, groups) in zip(cues, values):
                value = separator.join(groups)
                family = label.lower()
                for surface, template in templates.items():
                    name = separator_name + "_" + surface
                    emit("A", family, name, index, template, value, label, cue)
                    emit("R", family, name, index, template + ("\nAgain: {V}" if partition == "dev" else "\nCopy = {V}"),
                         value, label, cue)
            # Same invisible characters in benign source text: words, technical
            # references, arithmetic, public paths and incomplete mail fragments.
            benign = {
                "prose": separator.join(("co", "operative")) if partition == "dev" else separator.join(("inter", "national")),
                "code": separator.join(("AB", "12", "CD", "34")) if partition == "dev" else separator.join(("XY", "56", "ZT", "78")),
                "number": separator.join(("2", "4", "6")) if partition == "dev" else separator.join(("3", "5", "7")),
                "path": separator.join(("/public", "manual")) if partition == "dev" else separator.join(("/assets", "guide")),
                "mail_fragment": separator.join(("build", "@")) if partition == "dev" else separator.join(("release", "@")),
            }
            if separator_name == "bom":
                benign["leading_code"] = separator + (
                    "const answer = 42;" if partition == "dev" else "let total = 17;")
                benign["embedded_code"] = (
                    'const label = "hello' + separator + 'world";' if partition == "dev"
                    else 'let title = "public' + separator + 'manual";')
            for family, value in benign.items():
                template = "Public sample: {V}; complete." if partition == "dev" else "Documentation only = {V}. Done."
                emit("D", family, separator_name + "_" + family, index, template, value, None)
            # Format-matched benign twins remain invalid after any mapping:
            # an unassignable country code, unknown IBAN prefix, wrong card
            # checksum, numeric schema placeholder and incomplete mailbox.
            card = "".join(values[3][1])
            wrong_card = card[:-1] + str((int(card[-1]) + 1) % 10)
            twins = {
                "invisible_card_twin": separator.join(api._by_four(wrong_card)),
                "invisible_tax_twin": separator.join(("000", "000", "0000")),
                "invisible_phone_twin": separator.join(("+0", *phone[1:])),
                "invisible_iban_twin": separator.join(("XY00", *values[1][1][1:])),
                "invisible_email_twin": separator.join((email[:3], email[3:] + "@")),
            }
            for family, value in twins.items():
                template = "Schema example: {V}; incomplete." if partition == "dev" else "Public format fixture = {V}. Not assigned."
                emit("D", family, separator_name + "_" + family, index, template, value, None)
    return result
