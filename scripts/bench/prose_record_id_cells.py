"""CC0 EN/DE prose record IDs. Ownership, not the spelling of a code, determines gold."""

SURFACE_PREFIX = "prose_record_id_"

# Separate authored contexts, not copies of a template with a swapped ID.
PERSONAL = (
    # language, family, cue, development wording, held-out wording
    ("en", "record_id", "case", "Please reopen my case {V} before the hearing.",
     "The patient asked about case {V} during intake."),
    ("en", "record_id", "claim", "I filed claim {V} after my injury.",
     "Please send my claim number {V} to the adjuster."),
    ("en", "record_id", "member", "My member ID is {V}; can you renew my card?",
     "I registered under member number {V} last year."),
    ("en", "employee_id", "employee", "Use employee ID {V} for my payroll correction.",
     "The badge lists my employee ID as {V}."),
    ("en", "customer_id", "customer", "I provided my customer ID: {V}.",
     "Could you check my customer ID {V} again?"),
    ("en", "record_id", "mrn", "My discharge sheet gives MRN {V}.",
     "Please retrieve the patient's chart (MRN: {V})."),
    ("de", "record_id", "fallnummer", "Für meinen Antrag gilt die Fallnummer {V}.",
     "Meine Fallnummer lautet {V}; bitte den Bescheid zuordnen."),
    ("de", "record_id", "fall_nr", "Bitte meine Akte unter Fall-Nr. {V} ergänzen.",
     "Auf meinem Entlassbrief steht Fall-Nr.: {V}."),
    ("de", "record_id", "fn", "Die Klinik führt meinen Fall unter FN {V}.",
     "Für meine Aufnahme wurde die FN: {V} vergeben."),
    ("de", "customer_id", "kundennummer", "Meine Kundennummer ist {V}, nicht die Rechnungsnummer.",
     "Bitte buchen Sie unter Kundennummer {V}."),
    ("de", "record_id", "versichertennummer", "Laut Bescheid ist meine Versichertennummer {V}.",
     "Für den Erstattungsantrag: Versichertennummer {V}."),
)

UNCUED = (
    ("en", "employee_id", "I clocked in with {V} this morning.", "Pay me under {V} for that shift."),
    ("de", "employee_id", "Ich stemple mit {V} ein.", "Meine Schichten laufen unter {V}."),
    ("en", "record_id", "I signed in as {V} yesterday.", "I used {V} to sign in on Monday."),
    ("de", "record_id", "Ich melde mich mit {V} an.", "Gestern war ich als {V} angemeldet."),
)

PUBLIC_CODES = {
    "en": ("release", "Release {V} fixes the member ID lookup.",
     "Customer ID validation shipped in release {V}."),
    "de": ("reference", "Die Referenz {V} bezeichnet das Formular für die Fallnummer.",
     "Keine Versichertennummer nötig; der Katalog führt das Blankoformular unter {V}."),
}

NO_VALUE = (
    ("en", "selector", 'Use the CSS selector [name="customer_id"] in the form.',
     'The lookup path is $.claim.member_id, not a member ID.'),
    ("de", "selector", 'Mit #fallnummer wird das Eingabefeld ausgewählt.',
     'Der XPath //input[@name="Versichertennummer"] wählt das Textfeld aus.'),
    ("en", "missing", "My case number is still pending; can I submit the form?",
     "The employee ID is missing from the payroll export."),
    ("de", "missing", "Die Fall-Nr. wird erst nach Prüfung vergeben.",
     "Meine Kundennummer liegt mir noch nicht vor."),
    ("en", "case_selector", 'The switch arm case "pending": retries the request.',
     'Route closed items with case "archived": rather than the default arm.'),
)


def records(api, partition):
    rng = api.Rng(api.PARTITION_SEEDS[partition], "prose-record-id-v32")
    dev = partition == "dev"
    prefixes = ("AX", "pQ", "LUM") if dev else ("VT", "hK", "MER")
    result = []

    def emit(layer, language, family, surface, index, template, fields):
        text, gold, decoys = api._fill_with_decoys(template, fields)
        result.append(api.Record(
            uid=f"agentic-{partition}-{layer}-prose-id-{language}-{surface}-{index}",
            partition=partition, layer=layer, language=language, region="DE" if language == "de" else "US",
            family=family, surface=SURFACE_PREFIX + surface,
            group=f"{partition}-prose-id-{language}-{surface}-{index}",
            template=f"prose-id/{partition}/{language}/{surface}",
            validity=api.BENIGN if layer == "D" else api.UNCHECKED, text=text, gold=gold, decoys=decoys))

    def selector(surface, index, value):
        # The same bytes select a public artifact, not a person. Neither the key,
        # the dotted tool name, nor the path around the key is gold.
        template = ('{"name":"release.catalog","arguments":{"tag":"{V}","selector":"assets/{V}/notes"}}' if dev else
                    '{"tool":"docs.lookup","input":{"reference":"{V}","path":"/manuals/{V}/index.md"}}')
        emit("D", "en", "tool_selector", surface, index, template, {"V": (value, None)})
        if "/" not in value and not value.isdigit():
            name = ('{"name":"{V}","arguments":{"format":"text"}}' if dev else
                    '{"type":"tool_use","name":"{V}","input":{"verbose":false}}')
            emit("D", "en", "tool_name", surface + "_name", index, name, {"V": (value, None)})

    # Short and compound keys: a digit run or a prefix up to the first separator
    # is not enough to restore a compound identifier. The outer prose is not gold.
    for index in range(api.DOCS_PER_FAMILY):
        serial = ("3" if dev else "8") + rng.digits(rng.between(3, 6))
        for lang, cue, template in (
            ("en", "case_digits", "I'm calling about my case {V}." if dev else "Can you review my case {V} today?"),
            ("de", "fall_digits", "Meine Fall-Nr.{V} steht oben." if dev else "Zu meiner Fall-Nr.{V} fehlt der Befund."),
        ):
            emit("A", lang, "record_id", cue, index, template, {"V": (serial, "RECORD_ID")})
        selector("numeric_selector", index, serial)
        for surface, development, held_out in (
            ("switch", "switch (opcode) { case {V}: flush(); break; }",
             "switch (status) { case {V}: return retry(); default: return stop(); }"),
            ("test_case", "The regression suite includes test case {V} for empty input.",
             "Please rerun test case {V} after updating the parser."),
        ):
            emit("D", "en", "public_case", surface, index, development if dev else held_out, {"V": (serial, None)})
        for shape, pattern in (("compact", "{p}{n}"), ("hyphen", "{p}-{n}"),
                               ("underscore", "{p}_{n}_{s}"), ("slash", "{p}{n}/{s}"),
                               ("mixed", "{p}-{n}/{s}")):
            def identifier(prefix):
                return pattern.format(p=prefix, n=rng.digits(rng.between(3, 7)), s=rng.choice("ABCD") + rng.digits(2))

            for lang, family, cue, development, held_out in PERSONAL:
                emit("A", lang, family, cue + "_" + shape, index, development if dev else held_out,
                     {"V": (identifier(rng.choice(prefixes)), family.upper())})
            for lang, family, development, held_out in UNCUED:
                prefix = ("EMP" if dev else "STAFF") if family == "employee_id" else ("ACCT" if dev else "KTO")
                value = identifier(prefix)
                surface = f"uncued_{family}_{shape}"
                emit("A", lang, family, surface, index, development if dev else held_out, {"V": (value, family.upper())})
                selector(f"{lang}_{surface}_selector", index, value)
                cue, public_dev, public_test = PUBLIC_CODES[lang]
                emit("D", lang, "public_reference", surface + "_" + cue, index, public_dev if dev else public_test,
                     {"V": (value, None)})
    for lang, cue, development, held_out in NO_VALUE:
        emit("D", lang, "schema_or_prose", cue, 0, development if dev else held_out, {})
    return result
