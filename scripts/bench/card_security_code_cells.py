"""CC0 synthetic payment cards and separately owned short numbers (generator v32)."""

SURFACE_PREFIX = "card_security_code_"
REFERENCE_TWIN = "card_code_reference_twin"
LAYOUTS = ((4, 4, 4), (4, 4, 4, 4), (4, 6, 5), (4, 6, 4), (4, 4, 4, 4, 3),
           *((n,) for n in range(12, 20)))

# Each partition has its own cues and prose. A public number immediately
# after a PAN needs an explicit owner too: a bare adjacent code is not benign.
COPY = {
    ("dev", "en"): ("Card: {P}", "{N} (page)", "Invoice {N}",
                    "Invoice: {R} {N} (page)", "CVV help, see page {N}",
                    "Cost: USD {M}", "CVC form version {N}"),
    ("test", "en"): ("My debit card number is {P}", "{N} (invoice number)", "Page {N}",
                     "Batch {R} {N} (sheet number)", "CVC instructions in section {N}",
                     "Subtotal: ${M}", "CVV validation error count: {N}"),
    ("dev", "de"): ("Kreditkarte: {P}", "{N} (Seite)", "Rechnung {N}",
                    "Beleg: {R} {N} (Seite)", "CVV fehlt; siehe Seite {N}",
                    "Preis: {M} EUR", "CVC-Formularversion: {N}"),
    ("test", "de"): ("Für die Zahlung: Kartennummer {P}", "{N} (Belegnummer)", "Seite {N}",
                     "Charge {R} {N} (Blattnummer)", "CVC-Hilfe im Abschnitt {N}",
                     "Summe: {M} Euro", "CVV-Prüfung: {N} Fehlversuche"),
}


def records(api, partition):
    rng = api.Rng(api.PARTITION_SEEDS[partition], "card-security-code-v32")
    result = []

    def emit(layer, family, surface, group, validity, template, fields):
        text, gold, decoys = api._fill_with_decoys(template, fields)
        result.append(api.Record(
            uid=f"{group}-{surface}-{validity}", partition=partition, layer=layer,
            family=family, surface=SURFACE_PREFIX + surface, validity=validity,
            group=group, template=f"{partition}/{language}/{template}", language=language,
            region="DE" if language == "de" else "US", text=text, gold=gold, decoys=decoys))

    for language in ("en", "de"):
        cue, post, pre, reference, loose, amount, metadata = COPY[partition, language]
        for widths in LAYOUTS:
            layout = "x".join(map(str, widths))
            length = sum(widths)
            for size in (3, 4):
                group = f"agentic-{partition}-{language}-cardcode-{layout}-{size}"
                # Stay in the partition's numeric pool; test also owns the
                # three-digit house/flat range 100-299 in the address cells.
                code = rng.choice("034" if partition == "dev" else "56789") + rng.digits(size - 1)
                if length >= 16:
                    prefix = rng.choice(("22", "35", "4", "51", "62"))
                elif length < 14 or (len(widths) == 1 and (size == 3) == (language == "en")):
                    prefix = rng.choice(("50", "56", "62"))  # Short Maestro PAN.
                else:
                    prefix = "36" if length == 14 else "37"
                body = prefix + rng.digits(length - len(prefix) - 1)
                valid_pan = body + api.luhn_check_digit(body)
                separator = rng.choice((" ", "-", "\u00a0", "\u202f"))
                for validity, digits in ((api.VALID, valid_pan), (api.INVALID, api._bump_last_digit(valid_pan))):
                    pan = separator.join(digits[sum(widths[:i]):sum(widths[:i + 1])] for i in range(len(widths)))
                    fields = {"P": (pan, "CREDITCARDNUMBER"), "N": (code, "SECURITYTOKEN")}
                    label = "CVV" if (length + size) % 2 else "CVC"
                    for form, suffix in (("bare", " {N}"), ("label", f"; {label}: {{N}}"),
                                         ("postlabel", f" {{N}} ({label})")):
                        emit("A", "card_and_code", f"{layout}_{form}", group, validity, cue + suffix, fields)
                    fields["N"] = (code, api.DECOY_PREFIX + "public_number")
                    for form, suffix in (("post_owner", post), ("pre_owner", pre)):
                        emit("A", "card_and_code", f"{layout}_{form}", group, validity,
                             cue + " " + suffix, fields)
                    if validity == api.INVALID:
                        fields["R"] = (pan, api.DECOY_PREFIX + "public_reference")
                        emit("D", REFERENCE_TWIN, f"{layout}_public", group, api.BENIGN, reference, fields)

        for size in (3, 4):
            group = f"agentic-{partition}-{language}-public-{size}"
            number = rng.digits(size)
            decimal = "." if language == "en" else ","
            fields = {"N": (number, api.DECOY_PREFIX + "public_number"),
                      "M": (number + decimal + rng.digits(2), api.DECOY_PREFIX + "amount"),
                      "R": ("LOT-" + rng.digits(7), api.DECOY_PREFIX + "reference")}
            for form, template in (("reference", reference), ("loose", loose),
                                   ("amount", amount), ("metadata", metadata)):
                emit("D", "public_short_number", form, group, api.BENIGN, template, fields)
    return result
