"""Fabricated values only; parent templates/translations/variants never cross splits."""

import hashlib
import json
import unicodedata
from pathlib import Path


VALUES = {
    "train": {"names": ["Mira Velloran", "Taro Nelvick", "Ona Zelmori", "Fero Bandelis", "Ari Kendovic", "Nela Tormavi", "Sivo Rendolan", "Lumi Fenwick"], "words": ["Baker", "Winter", "Green", "Brown", "Hill", "King", "Bell", "Stone"], "ids": ["80421739", "63190528", "75021693", "14285360", "92410753", "36071284", "41768029", "68293510"]},
    "dev": {"names": ["Lena Quenwick", "Niko Faldoren", "Ena Vorlund", "Ralo Sandek", "Una Malvori", "Kivo Brendali", "Tena Rulvick", "Jori Naskel"], "words": ["Rose", "Summer", "Grace", "Faith", "Hope", "Dawn", "Lake", "Woods"], "ids": ["50731846", "92064183", "19074625", "48231569", "61389042", "74920816", "26815793", "85603421"]},
    "test": {"names": ["Sora Melvanto", "Ivo Darnelis", "Yuna Kervali", "Elio Nortavi", "Vera Pellund", "Milo Quenset", "Rina Vaskeli", "Tova Nelvarin"], "words": ["May", "March", "Will", "Mark", "Bill", "Page", "Cook", "West"], "ids": ["47280631", "85139204", "30968172", "72641908", "16402783", "58093614", "93718265", "24570396"]},
}

# Each list has the same rubric, but separate template parents in each split.
# The translated templates in a parent are one unit, never separate samples for splitting.
# Rubric: person, organisation, software, ordinary word, person-in-code,
# code symbol, customer identifier, technical number.
TEMPLATES = {
    "train": {
        "en": ["Our customer {v} said she would call.", "The company {v} sells tools.", "Load the language model {v}.", "The word {v} is printed in the dictionary.", 'contact = {{"person": "{v}"}}', 'class {v}: pass', "Customer account identifier: {v}", "Build counter: {v}"],
        "de": ["Unsere Kundin {v} sagte, sie werde anrufen.", "Die Firma {v} verkauft Werkzeug.", "Lade das Sprachmodell {v}.", "Das Wort {v} steht im Wörterbuch.", 'kontakt = {{"person": "{v}"}}', 'Klassenname im Code: {v}', "Kundenkontokennung: {v}", "Build-Zähler: {v}"],
        "es": ["Nuestra clienta {v} dijo que llamaría.", "La empresa {v} vende herramientas.", "Carga el modelo de lenguaje {v}.", "La palabra {v} aparece en el diccionario.", 'contacto = {{"persona": "{v}"}}', 'Nombre de clase en código: {v}', "Identificador de cuenta del cliente: {v}", "Contador de compilación: {v}"],
        "fr": ["Notre cliente {v} a dit qu'elle appellerait.", "La société {v} vend des outils.", "Charge le modèle de langage {v}.", "Le mot {v} figure dans le dictionnaire.", 'contact = {{"personne": "{v}"}}', 'Nom de classe dans le code : {v}', "Identifiant du compte client : {v}", "Compteur de compilation : {v}"],
    },
    "dev": {
        "en": ["The witness, {v}, described what he saw.", "Shares in {v} rose after its merger.", "Use {v} as the API model parameter.", "Translate the vocabulary item {v} into French.", 'patient_name = "{v}"', 'import {v} from "./types";', "Private employee record number = {v}", "Pixels in the image = {v}"],
        "de": ["Der Zeuge {v} beschrieb, was er sah.", "Die Aktien von {v} stiegen nach der Fusion.", "Verwende {v} als API-Modellparameter.", "Übersetze den Vokabeleintrag {v} ins Französische.", 'patientenname = "{v}"', 'import {v} from "./typen";', "Private Personalaktennummer = {v}", "Pixel im Bild = {v}"],
        "es": ["El testigo {v} describió lo que vio.", "Las acciones de {v} subieron tras la fusión.", "Usa {v} como parámetro de modelo de la API.", "Traduce la entrada de vocabulario {v} al francés.", 'nombre_paciente = "{v}"', 'import {v} from "./tipos";', "Número privado del expediente del empleado = {v}", "Píxeles de la imagen = {v}"],
        "fr": ["Le témoin {v} a décrit ce qu'il a vu.", "Les actions de {v} ont monté après sa fusion.", "Utilise {v} comme paramètre de modèle de l'API.", "Traduis l'entrée de vocabulaire {v} en français.", 'nom_patient = "{v}"', 'import {v} from "./types";', "Numéro privé du dossier de l'employé = {v}", "Pixels dans l'image = {v}"],
    },
    "test": {
        "en": ["Tomorrow's appointment is with {v}; please notify him.", "The board of {v} approved its annual accounts.", 'The configuration requires {{"model": "{v}"}}.', "In this spelling exercise, underline {v}.", '{{"shipping_recipient": "{v}"}}', 'const {v} = new Parser();', "The patient's confidential registry key is {v}.", "The benchmark completed {v} iterations."],
        "de": ["Der morgige Termin ist mit {v}; bitte benachrichtige ihn.", "Der Vorstand von {v} genehmigte den Jahresabschluss.", 'Die Konfiguration benötigt {{"model": "{v}"}}.', "Unterstreiche in dieser Rechtschreibübung {v}.", '{{"paketempfänger": "{v}"}}', 'const {v} = new Parser();', "Der vertrauliche Registerschlüssel des Patienten ist {v}.", "Der Benchmark hat {v} Iterationen abgeschlossen."],
        "es": ["La cita de mañana es con {v}; avísale.", "El consejo de {v} aprobó sus cuentas anuales.", 'La configuración requiere {{"model": "{v}"}}.', "En este ejercicio de ortografía, subraya {v}.", '{{"destinatario_envío": "{v}"}}', 'const {v} = new Parser();', "La clave confidencial del registro del paciente es {v}.", "El benchmark completó {v} iteraciones."],
        "fr": ["Le rendez-vous de demain est avec {v} ; préviens-le.", "Le conseil de {v} a approuvé ses comptes annuels.", 'La configuration exige {{"model": "{v}"}}.', "Dans cet exercice d'orthographe, souligne {v}.", '{{"destinataire_colis": "{v}"}}', 'const {v} = new Parser();', "La clé confidentielle du registre du patient est {v}.", "Le benchmark a terminé {v} itérations."],
        "it": ["L'appuntamento di domani è con {v}; avvisalo.", "Il consiglio di {v} ha approvato il bilancio annuale.", 'La configurazione richiede {{"model": "{v}"}}.', "In questo esercizio di ortografia, sottolinea {v}.", '{{"destinatario_pacco": "{v}"}}', 'const {v} = new Parser();', "La chiave riservata del registro del paziente è {v}.", "Il benchmark ha completato {v} iterazioni."],
        "pt": ["A consulta de amanhã é com {v}; avisa-o.", "O conselho da {v} aprovou as contas anuais.", 'A configuração exige {{"model": "{v}"}}.', "Neste exercício de ortografia, sublinha {v}.", '{{"destinatário_encomenda": "{v}"}}', 'const {v} = new Parser();', "A chave confidencial do registo do paciente é {v}.", "O benchmark concluiu {v} iterações."],
    },
}

FAMILIES = ["person", "organisation", "model_id", "ordinary_word", "person_in_code", "code_symbol", "private_id", "technical_number"]
DOMAINS = ["prose", "business", "config", "language_exercise", "config", "code", "records", "technical"]


def make_row(split, language, family, variant, value, template, *, evaluation=None):
    text = template.format(v=value)
    start = text.index(value)
    positive = family in {"person", "person_in_code", "private_id"}
    return {
        "id": f"{split}/{language}/{family}/{variant}/{evaluation or 'base'}",
        "split": split, "evaluation": evaluation or ("language_holdout" if language in {"it", "pt"} else split),
        "parent": f"{split}/{family}/{variant}", "template_parent": f"{split}/{family}",
        "language": language, "family": family, "domain": DOMAINS[FAMILIES.index(family)],
        "target": "identifier" if family in {"private_id", "technical_number"} else "name",
        "entity": "person" if family in {"person", "person_in_code"} else ("identifier" if family == "private_id" else "other"),
        "label": int(positive), "value": value, "text": text,
        "span": [start, start + len(value)], "byte_span": [len(text[:start].encode()), len(text[:start + len(value)].encode())],
        "reality": "fabricated; contextual reference only", "policy": "protect person references and private record identifiers only",
    }


def rows():
    result = []
    for split, languages in TEMPLATES.items():
        values = VALUES[split]
        for language, templates in languages.items():
            for index, (family, template) in enumerate(zip(FAMILIES, templates, strict=True)):
                for variant in range(len(values["names"])):
                    value = (values["ids"][variant] if index >= 6 else values["words"][variant] if index == 3 else
                             values["names"][variant].split()[0] if index == 5 else
                             f"{values['names'][variant].split()[0].lower()}-sonnet-4-{variant+1}" if index == 2 else
                             values["names"][variant])
                    result.append(make_row(split, language, family, variant, value, template))
    # Stress variants derive ONLY from test parents. They cannot tune thresholds.
    for original in list(result):
        if original["split"] != "test" or original["language"] not in {"en", "de"}:
            continue
        if original["family"] not in {"person", "person_in_code", "model_id", "code_symbol"}:
            continue
        for style in ["lower", "upper", "nbsp", "zwsp"]:
            value = original["value"]
            value = value.lower() if style == "lower" else value.upper() if style == "upper" else value.replace(" ", "\u00a0") if style == "nbsp" else "\u200b".join(value.split(" "))
            template = original["text"].replace(original["value"], "{v}").replace("{", "{{").replace("}", "}}")
            template = template.replace("{{v}}", "{v}")
            row = make_row("test", original["language"], original["family"], original["id"].split('/')[3] + "-" + style, value, template, evaluation="stress")
            row["parent"] = original["parent"]
            result.append(row)
    validate(result)
    return result


def validate(items):
    assert len({r["id"] for r in items}) == len(items)
    for key in ["parent", "template_parent"]:
        assignments = {}
        for r in items:
            assignments.setdefault(r[key], set()).add(r["split"])
        assert all(len(v) == 1 for v in assignments.values()), f"split leakage: {key}"
    for i, left in enumerate(VALUES):
        for right in list(VALUES)[i + 1:]:
            assert not set(sum(VALUES[left].values(), [])).intersection(sum(VALUES[right].values(), []))
    texts = {}
    for r in items:
        key = unicodedata.normalize("NFKC", r["text"]).casefold()
        texts.setdefault(key, set()).add(r["split"])
        a, b = r["span"]
        x, y = r["byte_span"]
        assert r["text"][a:b] == r["value"]
        assert r["text"].encode()[x:y].decode() == r["value"]
    assert all(len(v) == 1 for v in texts.values())


def write(path: Path):
    data = "".join(json.dumps(row, ensure_ascii=False, sort_keys=True) + "\n" for row in rows())
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(data)
    return hashlib.sha256(data.encode()).hexdigest()


if __name__ == "__main__":
    import argparse
    parser = argparse.ArgumentParser()
    parser.add_argument("output", type=Path)
    print(write(parser.parse_args().output))
