"""CC0 producer-owned wrappers and genuine CSV-encoded complete labelled cells."""
import csv
import io
import json

from field_wrappers import WRAPPERS
from record_id_boundary_cells import FIELDS, LABELS, PUBLIC, encoded_content


def csv_cell(text, fully_quoted):
    output = io.StringIO(newline="")
    csv.writer(output, quoting=csv.QUOTE_ALL if fully_quoted else csv.QUOTE_MINIMAL,
               lineterminator="\r\n").writerow([text, "{W}"])
    return output.getvalue()


def templates(opening, closing, shape, value=None):
    declaration = "{C}=" + opening + "{V}" + closing
    if shape == "record":
        return declaration + "\npublic_version={W}", 0, False
    if shape.startswith("json_depth"):
        depth = int(shape.removeprefix("json_depth"))
        text = declaration
        for _ in range(depth):
            text = json.dumps({"note": text, "public_version": "{W}"}, ensure_ascii=False)
        return text, depth, False
    if shape.startswith("csv_"):
        complete = declaration.replace("{V}", value) if value is not None else declaration
        template = csv_cell(complete, shape == "csv_full")
        quoted = template.startswith('"')
        if value is not None:
            raw = value.replace('"', '""') if quoted else value
            template = template.replace(raw, "{V}")
        return template, 0, quoted
    if shape == "markdown":
        return "| " + declaration + " | {W} |", 0, False
    if shape == "yaml":
        return "{C}: " + opening + "{V}" + closing + "\npublic_version: {W}", 0, False
    if shape == "sql_literal":
        # An outer SQL literal doubles all apostrophes in the complete label,
        # rather than pretending a SQL assignment is itself an enclosing string.
        return "SELECT '" + declaration.replace("'", "''") + "', '{W}';", 0, "sql"
    raise AssertionError(shape)


def records(api, partition):
    result = []
    public_key, _, word = PUBLIC[partition]
    lead = "5" if partition == "dev" else "7"
    for family in LABELS:
        key = FIELDS[partition][family][0]
        for index, (opening, closing) in enumerate(WRAPPERS):
            shapes = ["record", *(f"json_depth{depth}" for depth in range(1, 10)),
                      "csv_minimal", "csv_full", "markdown", "yaml", "sql_literal"]
            for shape in shapes:
                variants = ("plain", "double", "single") if shape.startswith("csv_") or shape == "sql_literal" else ("plain",)
                for variant in variants:
                    rng = api.Rng(api.PARTITION_SEEDS[partition], f"recordids-wrappers/{family}/{index}/{shape}/{variant}")
                    value = "NX" + lead + rng.digits(5)
                    if variant != "plain":
                        quote = '"' if variant == "double" else "'"
                        value = value[:4] + quote + value[4:]
                    if opening == closing:
                        value = value.replace(opening, opening * 2)
                    template, depth, encoded_csv = templates(opening, closing, shape, value)
                    if encoded_csv == "sql":
                        value = value.replace("'", "''")
                    elif encoded_csv:
                        value = value.replace('"', '""')
                    elif depth:
                        value = encoded_content(value, depth - 1)
                    fields = {"C": (key, None), "V": (value, LABELS[family]),
                              "W": (word, "decoy:public_column")}
                    for layer in ("A", "D", "R"):
                        actual = fields if layer != "D" else {**fields, "C": (public_key, None),
                                  "V": (value, "decoy:public_reference")}
                        text, gold, decoys = api._fill_with_decoys(
                            template if layer != "R" else "Lookup {V} again.\n" + template, actual)
                        result.append(api.Record(
                            uid=f"agentic-{partition}-{layer}-wrappers-{family}-{index:03d}-{shape}-{variant}",
                            partition=partition, layer=layer, family=family,
                            surface=f"recordids_wrappers_{index:03d}_{shape}_{variant}",
                            validity=api.BENIGN if layer == "D" else api.UNCHECKED,
                            group=f"{partition}-recordids-wrappers-{family}-{index:03d}",
                            template=f"recordids-wrappers/{family}/{index}/{shape}/{variant}/{partition}",
                            language="en", region="US", text=text, gold=gold, decoys=decoys))
    return result
