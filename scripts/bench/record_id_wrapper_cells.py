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
    return (result + multiline_records(api, partition) + structured_quote_records(api, partition)
            + backslash_records(api, partition) + absent_records(api, partition) + mysql_records(api, partition) + sql_identifier_records(api, partition))


def multiline_records(api, partition):
    """Raw record lines inside a verified enclosing cell are private content."""
    result = []
    public_key, _, word = PUBLIC[partition]
    for family in LABELS:
        key = FIELDS[partition][family][0]
        for index, opener in enumerate(('“', "'", '"', '§', '`')):
            for shape in ('internal_lf', 'initial_lf', 'prior_cell_lf'):
                rng = api.Rng(api.PARTITION_SEEDS[partition], f'recordids-unpaired-multiline/{family}/{index}/{shape}')
                serial = ('5' if partition == 'dev' else '7') + rng.digits(5)
                value = '\nNX' + serial if shape == 'initial_lf' else 'NX' + serial[:3] + '\n' + serial[3:]
                for dialect in ('csv_minimal', 'csv_full', 'sql_literal', 'open_cell_json'):
                    declaration = ('metadata: ' + word + '\n' if shape == 'prior_cell_lf' else '') + '{C}=' + opener + value
                    if dialect == 'open_cell_json':
                        note = '"' + declaration.replace('"', '""')
                        template = json.dumps({'note':note,'public_version':'{W}'}, ensure_ascii=False)
                        raw = encoded_content(value, 0)
                    elif dialect.startswith('csv_'):
                        template = csv_cell(declaration, dialect == 'csv_full')
                        raw = value.replace('"', '""')
                    else:
                        template = "SELECT '" + declaration.replace("'", "''") + "', '{W}';"
                        raw = value.replace("'", "''")
                    template = template.replace(raw, '{V}')
                    fields = {'C': (key, None), 'V': (raw, LABELS[family]),
                              'W': (word, 'decoy:public_column')}
                    for layer in ('A', 'D', 'R'):
                        actual = fields if layer != 'D' else {**fields, 'C': (public_key, None),
                                  'V': (raw, 'decoy:public_reference')}
                        text, gold, decoys = api._fill_with_decoys(
                            template if layer != 'R' else 'Lookup {V} again.\n' + template, actual)
                        result.append(api.Record(
                            uid=f'agentic-{partition}-{layer}-wrappers-multiline-{family}-{index}-{dialect}-{shape}',
                            partition=partition, layer=layer, family=family,
                            surface=f'recordids_wrappers_multiline_{index}_{dialect}_{shape}',
                            validity=api.BENIGN if layer == 'D' else api.UNCHECKED,
                            group=f'{partition}-recordids-wrappers-multiline-{family}-{index}',
                            template=f'recordids-wrappers-multiline/{family}/{index}/{dialect}/{shape}/{partition}',
                            language='en', region='US', text=text, gold=gold, decoys=decoys))
    return result


def structured_quote_records(api, partition):
    """Encoded triple scalars and genuinely empty private-cue counterweights."""
    result = []
    public_key, _, word = PUBLIC[partition]
    for family in LABELS:
        key = FIELDS[partition][family][0]
        for quote_name, quote in (('single', "'"), ('double', '"')):
            rng = api.Rng(api.PARTITION_SEEDS[partition], f'recordids-structured/{family}/{quote_name}')
            first = ('5' if partition == 'dev' else '7') + rng.digits(5)
            second = ('5' if partition == 'dev' else '7') + rng.digits(5)
            for kind, delimiter, value, layers in (
                ('triple', quote * 3, 'NX' + first + quote + '\nÉ' + second + quote * 2, ('A','D','R')),
                ('empty_single', quote, '', ('D',)),
                ('empty_triple', quote * 3, '', ('D',)),
            ):
                for dialect in ('csv_minimal','csv_full','sql_literal'):
                    declaration = '{C}=' + delimiter + value + delimiter
                    if dialect.startswith('csv_'):
                        template = csv_cell(declaration, dialect == 'csv_full')
                        raw = value.replace('"','""')
                    else:
                        template = "SELECT '" + declaration.replace("'","''") + "', '{W}';"
                        raw = value.replace("'","''")
                    if raw:
                        template = template.replace(raw,'{V}')
                    fields = {'C': (key,None), 'W': (word,'decoy:public_column')}
                    if raw:
                        fields['V'] = (raw,LABELS[family])
                    for layer in layers:
                        actual = fields
                        if layer == 'D' and raw:
                            actual = {**fields,'C':(public_key,None),'V':(raw,'decoy:public_reference')}
                        text,gold,decoys = api._fill_with_decoys(template if layer != 'R' else 'Lookup {V} again.\n'+template,actual)
                        result.append(api.Record(
                            uid=f'agentic-{partition}-{layer}-wrappers-structured-{family}-{kind}-{dialect}-{quote_name}',
                            partition=partition,layer=layer,family=family,
                            surface=f'recordids_wrappers_structured_{kind}_{dialect}_{quote_name}',
                            validity=api.BENIGN if layer=='D' else api.UNCHECKED,
                            group=f'{partition}-recordids-wrappers-structured-{family}-{kind}-{quote_name}',
                            template=f'recordids-wrappers-structured/{family}/{kind}/{dialect}/{quote_name}/{partition}',
                            language='en',region='US',text=text,gold=gold,decoys=decoys))
    return result


def backslash_records(api, partition):
    """CSV backslashes are literal, including before doubled quote bytes."""
    result = []
    public_key, _, word = PUBLIC[partition]
    for family in LABELS:
        key = FIELDS[partition][family][0]
        for index, (opening, closing) in enumerate((("'","'"), ('“','”'), ('`','`'))):
            rng = api.Rng(api.PARTITION_SEEDS[partition], f'recordids-backslash/{family}/{index}')
            serial = ('5' if partition == 'dev' else '7') + rng.digits(5)
            for shape in ('csv_minimal','csv_full',*(f'json_depth{n}' for n in range(1,10)),'json_string','json_unpaired_tail'):
                value = 'NX' + serial[:3] + '\\"' + (',' if shape == 'json_string' else '') + serial[3:]
                if shape == 'json_unpaired_tail':
                    value = 'NX' + serial + '"'
                declaration = '{C}=' + opening + value + ('' if shape == 'json_unpaired_tail' else closing)
                if shape in ('json_string','json_unpaired_tail'):
                    template = json.dumps(declaration, ensure_ascii=False) + '\npublic_version={W}'
                    raw = encoded_content(value, 0)
                else:
                    template = csv_cell('metadata: ' + word + '\n' + declaration, shape == 'csv_full')
                    raw = value.replace('"','""')
                    if shape.startswith('json_depth'):
                        depth = int(shape.removeprefix('json_depth'))
                        for _ in range(depth):
                            template = json.dumps({'note':template,'public_version':'{W}'}, ensure_ascii=False)
                        raw = encoded_content(raw, depth - 1)
                # Source escapes have been applied to the entire complete cell.
                template = template.replace(raw,'{V}')
                fields = {'C':(key,None),'V':(raw,LABELS[family]),'W':(word,'decoy:public_column')}
                for layer in ('A','D','R'):
                    actual = fields if layer != 'D' else {**fields,'C':(public_key,None),'V':(raw,'decoy:public_reference')}
                    text,gold,decoys = api._fill_with_decoys(template if layer!='R' else 'Lookup {V} again.\n'+template,actual)
                    result.append(api.Record(
                        uid=f'agentic-{partition}-{layer}-wrappers-backslash-{family}-{index}-{shape}',
                        partition=partition,layer=layer,family=family,
                        surface=f'recordids_wrappers_backslash_{index}_{shape}_value',
                        validity=api.BENIGN if layer=='D' else api.UNCHECKED,
                        group=f'{partition}-recordids-wrappers-backslash-{family}-{index}',
                        template=f'recordids-wrappers-backslash/{family}/{index}/{shape}/{partition}',
                        language='en',region='US',text=text,gold=gold,decoys=decoys))
    return result


def absent_records(api, partition):
    result = []
    for family in LABELS:
        for dialect in ('csv_minimal','csv_full','sql_literal'):
            declaration = '{C}='
            if dialect.startswith('csv_'):
                # The cue-only cell must itself have a quoted enclosing close.
                template = '"' + declaration + '","{W}"\r\n' if dialect=='csv_full' else '"' + declaration + '",{W}\r\n'
            else:
                template = "SELECT '" + declaration + "', '{W}';"
            text,gold,decoys = api._fill_with_decoys(template, {'C':(FIELDS[partition][family][0],None),
                    'W':(PUBLIC[partition][2],'decoy:public_column')})
            result.append(api.Record(
                uid=f'agentic-{partition}-D-wrappers-absent-{family}-{dialect}',partition=partition,layer='D',family=family,
                surface=f'recordids_wrappers_absent_{dialect}_value',validity=api.BENIGN,
                group=f'{partition}-recordids-wrappers-absent-{family}',
                template=f'recordids-wrappers-absent/{family}/{dialect}/{partition}',
                language='en',region='US',text=text,gold=gold,decoys=decoys))
    return result


def mysql_records(api, partition):
    result = []
    public_key, _, word = PUBLIC[partition]
    for family in LABELS:
        for index, (opening, closing) in enumerate((('“','”'),('`','`'))):
            rng = api.Rng(api.PARTITION_SEEDS[partition], f'recordids-mysql/{family}/{index}')
            serial = ('5' if partition == 'dev' else '7') + rng.digits(5)
            value = 'NX' + serial[:3] + "\\'" + serial[3:]
            template = "SELECT 'metadata: " + word + "\n{C}=" + opening + '{V}' + closing + "', '{W}';"
            fields = {'C':(FIELDS[partition][family][0],None),'V':(value,LABELS[family]),'W':(word,'decoy:public_column')}
            for layer in ('A','D','R'):
                actual = fields if layer!='D' else {**fields,'C':(public_key,None),'V':(value,'decoy:public_reference')}
                text,gold,decoys = api._fill_with_decoys(template if layer!='R' else 'Lookup {V} again.\n'+template,actual)
                result.append(api.Record(
                    uid=f'agentic-{partition}-{layer}-wrappers-mysql-{family}-{index}',partition=partition,layer=layer,family=family,
                    surface=f'recordids_wrappers_mysql_literal_{index}_value',validity=api.BENIGN if layer=='D' else api.UNCHECKED,
                    group=f'{partition}-recordids-wrappers-mysql-{family}-{index}',
                    template=f'recordids-wrappers-mysql/{family}/{index}/{partition}',language='en',region='US',text=text,gold=gold,decoys=decoys))
    return result


def sql_identifier_records(api, partition):
    result=[]
    public_key,_,word=PUBLIC[partition]
    for family in LABELS:
        for index,identifier in enumerate(('"quote \' hint"',"`quote ' hint`","[quote ' hint]")):
            rng=api.Rng(api.PARTITION_SEEDS[partition],f'recordids-sql-identifier/{family}/{index}')
            value='NX'+('5' if partition=='dev' else '7')+rng.digits(5)
            template="SELECT "+identifier+", {C}='{V}' FROM (SELECT '{W}' AS "+identifier+", '{W}' AS {C});"
            fields={'C':(FIELDS[partition][family][0],None),'V':(value,LABELS[family]),'W':(word,'decoy:public_column')}
            for layer in ('A','D','R'):
                actual=fields if layer!='D' else {**fields,'C':(public_key,None),'V':(value,'decoy:public_reference')}
                text,gold,decoys=api._fill_with_decoys(template if layer!='R' else 'Lookup {V} again.\n'+template,actual)
                result.append(api.Record(
                    uid=f'agentic-{partition}-{layer}-wrappers-sql-identifier-{family}-{index}',partition=partition,layer=layer,family=family,
                    surface=f'recordids_wrappers_sql_identifier_{index}_value',validity=api.BENIGN if layer=='D' else api.UNCHECKED,
                    group=f'{partition}-recordids-wrappers-sql-identifier-{family}-{index}',template=f'recordids-wrappers-sql-identifier/{family}/{index}/{partition}',
                    language='en',region='US',text=text,gold=gold,decoys=decoys))
    return result
