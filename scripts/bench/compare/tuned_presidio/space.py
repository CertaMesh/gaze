"""The declared search space for Presidio tuned for Gaze's own corpus.

This file is the whole space. It was committed before any candidate was scored,
and the result report pins its SHA-256, so the space cannot be widened after a
result is seen. Nothing here reads the corpus.

What the search may choose, per candidate:

- the NLP-engine NER (`ARTIFACT_NER`): spaCy large for every language, English
  `dslim/bert-base-NER` through Presidio's transformers engine (spaCy large for the
  other languages, as in the comparison's "strong" row), or neither;
- extra NER recognizers Presidio ships (`EXTRA_NER`), each off, English-only or
  on for every language: `HuggingFaceNerRecognizer` with the multilingual Davlan
  model (the NER model Gaze's own setup installs) or the OpenMed PII model (the
  one Presidio Research chose for its tuned setup), and `GLiNERRecognizer` with
  its default model;
- every predefined Presidio pattern recognizer, each off, on for its native
  languages, or on for every language;
- the custom recognizers in `CUSTOM_RECOGNIZERS`, each off or on;
- a score threshold per recognizer and entity from `THRESHOLD_GRID`
  (Presidio's native `score_thresholds`);
- the context enhancer (`CONTEXT_MODES`);
- an exact-match allow list learned from validation false positives
  (`ALLOW_LIST_MIN_DOCUMENTS`).

Selection sees the validation half only (`split_for_id`), and the test half is
never read until the choice is frozen. `search.py` walks the space by
deterministic coordinate descent (`SEARCH`) from each start in `STARTS`, under
each objective in `OBJECTIVES`.
"""

from __future__ import annotations

SPACE_VERSION = 1
LANGUAGES = ("en", "de", "nl", "fr", "pt")

#: Both objectives read validation metrics only, summed (pooled) over C/A/D/R under v3.
#: `leak-first` is exactly compare.py's `select_thresholds` rule. A free search can
#: drive it to "redact nearly everything", so `f2` (the panels' headline metric)
#: is searched too, and the chart shows whichever tuned row scores better on the
#: test half.
OBJECTIVES = {
    "leak-first": "fewest validation v3 leaked bytes, then fewest validation false-positive bytes",
    "f2": "highest validation v3 character-level F2 (micro over C/A/D/R), then fewest leaked bytes",
}

#: Recognizer-internal floors: a model below its floor is never recorded, so the
#: engine-level thresholds of `THRESHOLD_GRID` can only raise them.
THRESHOLD_GRID = (0.0, 0.05, 0.1, 0.15, 0.2, 0.25, 0.3, 0.35, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9, 1.0)

#: LemmaContextAwareEnhancer settings: (similarity factor, min score with context,
#: prefix words, suffix words). "off" leaves every score unchanged.
CONTEXT_MODES = {
    "off": (0.0, 0.0, 5, 0),
    "default": (0.35, 0.4, 5, 0),
    "wide": (0.35, 0.4, 10, 10),
}

#: An allow-list candidate is an exact result text that no validation gold byte
#: overlaps, predicted in at least this many validation documents.
ALLOW_LIST_MIN_DOCUMENTS = (None, 2, 3, 5, 10, 20)

ARTIFACT_NER = ("spacy", "dslim", "none")
EXTRA_NER_SCOPES = ("off", "en", "all")
PATTERN_SCOPES = ("off", "native", "all")

SPACY_WHEELS = {
    "en": "en_core_web_lg", "de": "de_core_news_lg", "nl": "nl_core_news_lg",
    "fr": "fr_core_news_lg", "pt": "pt_core_news_lg",
}

#: The comparison's strong row, unchanged.
DSLIM = {
    "repo": "dslim/bert-base-NER",
    "revision": "d1a3e8f13f8c3566299d95fcfc9a8d2382a9affc",
    "mapping": {"PER": "PERSON", "ORG": "ORGANIZATION", "LOC": "LOCATION", "MISC": "NRP"},
    "low_confidence_score_multiplier": 0.4,
    "low_score_entity_names": ["ORG"],
}

EXTRA_NER = {
    "davlan": {
        "recognizer": "HuggingFaceNerRecognizer",
        "repo": "Davlan/bert-base-multilingual-cased-ner-hrl",
        "revision": "e756de7f7b8f64fea0c3d7c3872f1322fab747b1",
        "label_mapping": {"PER": "PERSON", "ORG": "ORGANIZATION", "LOC": "LOCATION", "DATE": "DATE_TIME"},
        "aggregation_strategy": "simple",
        "floor": 0.05,
    },
    "openmed": {
        "recognizer": "HuggingFaceNerRecognizer",
        "repo": "OpenMed/OpenMed-PII-SuperClinical-Large-434M-v1",
        "revision": "df7af994d39d358e52f929ff1b3a40d894adf022",
        # Presidio Research's notebook 5 mapping, verbatim (presidio_research_repro.OPENMED_MAPPING).
        "label_mapping": "notebook5",
        "aggregation_strategy": "first",
        "floor": 0.05,
    },
    "gliner": {
        "recognizer": "GLiNERRecognizer",
        "repo": "urchade/gliner_multi_pii-v1",
        "revision": "1fcf13e85f4eef5394e1fcd406cf2ca9ea82351d",
        "floor": 0.1,
        "flat_ner": True,
    },
}

#: GLiNER model-card labels to Presidio entities. GLiNER sees every requested
#: entity name it does not map as an extra ad-hoc label, so `pool.py` adds an
#: identity entry for every other entity the pool can emit: the label set, and
#: therefore GLiNER's output, is then the same in every candidate.
GLINER_LABELS = {
    "person": "PERSON", "organization": "ORGANIZATION", "phone number": "PHONE_NUMBER",
    "address": "ADDRESS", "passport number": "PASSPORT", "email": "EMAIL_ADDRESS",
    "credit card number": "CREDIT_CARD", "social security number": "SSN",
    "health insurance id number": "HEALTH_INSURANCE_ID", "date of birth": "DATE_OF_BIRTH",
    "mobile phone number": "PHONE_NUMBER", "bank account number": "BANK_ACCOUNT",
    "medication": "MEDICATION", "cpf": "CPF", "driver's license number": "DRIVER_LICENSE",
    "tax identification number": "TAX_ID", "medical condition": "MEDICAL_CONDITION",
    "identity card number": "ID_CARD", "national id number": "NATIONAL_ID",
    "ip address": "IP_ADDRESS", "email address": "EMAIL_ADDRESS", "iban": "IBAN_CODE",
    "credit card expiration date": "CARD_EXPIRY", "username": "USERNAME",
    "health insurance number": "HEALTH_INSURANCE_ID", "registration number": "REGISTRATION_NUMBER",
    "student id number": "STUDENT_ID", "insurance number": "INSURANCE_NUMBER",
    "flight number": "FLIGHT_NUMBER", "landline phone number": "PHONE_NUMBER",
    "blood type": "BLOOD_TYPE", "cvv": "CVV", "reservation number": "RESERVATION_NUMBER",
    "digital signature": "DIGITAL_SIGNATURE", "social media handle": "USERNAME",
    "license plate number": "LICENSE_PLATE", "cnpj": "CNPJ", "postal code": "ZIP_CODE",
    "serial number": "SERIAL_NUMBER", "vehicle registration number": "LICENSE_PLATE",
    "credit card brand": "CARD_BRAND", "fax number": "PHONE_NUMBER", "visa number": "VISA_NUMBER",
    "insurance company": "ORGANIZATION", "identity document number": "ID_CARD",
    "transaction number": "TRANSACTION_NUMBER", "national health insurance number": "HEALTH_INSURANCE_ID",
    "cvc": "CVV", "birth certificate number": "BIRTH_CERTIFICATE", "train ticket number": "TICKET_NUMBER",
    "passport expiration date": "PASSPORT_EXPIRY",
}

#: Predefined recognizers that are not pattern or checksum recognizers (models,
#: cloud services, LLMs) never enter the pattern pool; models enter only through
#: `ARTIFACT_NER` and `EXTRA_NER`.
PREDEFINED_EXCLUDED = (
    "AzureAILanguageRecognizer", "AzureHealthDeidRecognizer", "AzureOpenAILangExtractRecognizer",
    "BasicLangExtractRecognizer", "LangExtractRecognizer", "GLiNERRecognizer",
    "HuggingFaceNerRecognizer", "MedicalNERRecognizer", "SpacyRecognizer", "StanzaRecognizer",
    "TransformersRecognizer",
)

#: Score of a deny-list match (Presidio's default is 1.0, which no threshold could
#: then remove; the recognizer's on/off switch still can).
DENY_LIST_SCORE = 0.6

#: The case-sensitive flags (MULTILINE | DOTALL) for custom patterns whose case matters;
#: Presidio's own default adds IGNORECASE.
CASE_SENSITIVE = 8 | 16

_SEP = r"[   ]"
_SEPD = r"[   .\-/]"
_EN_STREET = (
    r"(?:Street|St\.?|Road|Rd\.?|Avenue|Ave\.?|Lane|Ln\.?|Drive|Dr\.?|Crescent|Cres\.?|Terrace|"
    r"Grove|Close|Way|Court|Ct\.?|Boulevard|Blvd\.?|Place|Pl\.?|Square|Sq\.?|Parade|Highway|Hwy\.?|"
    r"Rise|Row|Walk|Gardens|Park|Mews|Hill|Quay|Esplanade|Circle|Parkway|Pkwy\.?|Trail|Heights)"
)
_DE_STREET = (
    r"(?:stra(?:ß|ss)e|str\.|weg|gasse|allee|platz|ring|damm|ufer|chaussee|steig|pfad|markt|"
    r"hof|berg|graben|zeile|kai|promenade|anger|stieg|twiete)"
)
_MONTHS = (
    r"(?:January|February|March|April|May|June|July|August|September|October|November|December|"
    r"Jan|Feb|Mar|Apr|Jun|Jul|Aug|Sep|Sept|Oct|Nov|Dec|Januar|Februar|März|Maerz|Juni|Juli|"
    r"Oktober|Dezember)"
)

US_STATES = (
    "Alabama", "Alaska", "Arizona", "Arkansas", "California", "Colorado", "Connecticut",
    "Delaware", "Florida", "Georgia", "Hawaii", "Idaho", "Illinois", "Indiana", "Iowa", "Kansas",
    "Kentucky", "Louisiana", "Maine", "Maryland", "Massachusetts", "Michigan", "Minnesota",
    "Mississippi", "Missouri", "Montana", "Nebraska", "Nevada", "New Hampshire", "New Jersey",
    "New Mexico", "New York", "North Carolina", "North Dakota", "Ohio", "Oklahoma", "Oregon",
    "Pennsylvania", "Rhode Island", "South Carolina", "South Dakota", "Tennessee", "Texas", "Utah",
    "Vermont", "Virginia", "Washington", "West Virginia", "Wisconsin", "Wyoming",
)
US_STATE_CODES = (
    "AL", "AK", "AZ", "AR", "CA", "CO", "CT", "DE", "FL", "GA", "HI", "ID", "IL", "IN", "IA",
    "KS", "KY", "LA", "ME", "MD", "MA", "MI", "MN", "MS", "MO", "MT", "NE", "NV", "NH", "NJ",
    "NM", "NY", "NC", "ND", "OH", "OK", "OR", "PA", "RI", "SC", "SD", "TN", "TX", "UT", "VT",
    "VA", "WA", "WV", "WI", "WY", "DC",
)
OTHER_REGIONS = (
    # Australia, New Zealand, Canada, United Kingdom, Ireland
    "New South Wales", "Victoria", "Queensland", "Western Australia", "South Australia",
    "Tasmania", "Northern Territory", "Australian Capital Territory", "Auckland", "Wellington",
    "Canterbury", "Otago", "Waikato", "Northland", "Southland", "Bay of Plenty", "Hawke's Bay",
    "Taranaki", "Manawatu-Whanganui", "Marlborough", "Nelson", "Tasman", "West Coast", "Gisborne",
    "Ontario", "Quebec", "British Columbia", "Alberta", "Manitoba", "Saskatchewan", "Nova Scotia",
    "New Brunswick", "Newfoundland and Labrador", "Prince Edward Island",
    "England", "Scotland", "Wales", "Northern Ireland", "Greater London", "Greater Manchester",
    "West Midlands", "West Yorkshire", "North Yorkshire", "South Yorkshire", "Merseyside", "Kent",
    "Essex", "Surrey", "Devon", "Cornwall", "Lancashire", "Hampshire", "Norfolk", "Suffolk",
    "Cheshire", "Somerset", "Dorset", "Oxfordshire", "Cambridgeshire", "Leinster", "Munster",
    "Connacht", "Ulster",
    # Germany, Austria, Switzerland
    "Baden-Württemberg", "Bayern", "Bavaria", "Berlin", "Brandenburg", "Bremen", "Hamburg",
    "Hessen", "Hesse", "Mecklenburg-Vorpommern", "Niedersachsen", "Lower Saxony",
    "Nordrhein-Westfalen", "North Rhine-Westphalia", "Rheinland-Pfalz", "Saarland", "Sachsen",
    "Saxony", "Sachsen-Anhalt", "Schleswig-Holstein", "Thüringen", "Thuringia", "Wien",
    "Niederösterreich", "Oberösterreich", "Steiermark", "Tirol", "Kärnten", "Salzburg",
    "Vorarlberg", "Burgenland", "Zürich", "Bern", "Luzern", "Uri", "Schwyz", "Zug", "Freiburg",
    "Solothurn", "Basel-Stadt", "Basel-Landschaft", "Schaffhausen", "Appenzell", "St. Gallen",
    "Graubünden", "Aargau", "Thurgau", "Tessin", "Waadt", "Wallis", "Neuenburg", "Genf", "Jura",
)
COUNTRIES = (
    "Germany", "Deutschland", "Austria", "Österreich", "Switzerland", "Schweiz", "France",
    "Frankreich", "Italy", "Italien", "Spain", "Spanien", "Portugal", "Netherlands", "Niederlande",
    "Holland", "Belgium", "Belgien", "Luxembourg", "Luxemburg", "Denmark", "Dänemark", "Sweden",
    "Schweden", "Norway", "Norwegen", "Finland", "Finnland", "Poland", "Polen", "Czech Republic",
    "Czechia", "Tschechien", "Hungary", "Ungarn", "Greece", "Griechenland", "Ireland", "Irland",
    "United Kingdom", "UK", "Great Britain", "Großbritannien", "England", "Scotland", "Schottland",
    "Wales", "United States", "USA", "U.S.", "America", "Amerika", "Vereinigte Staaten", "Canada",
    "Kanada", "Mexico", "Mexiko", "Brazil", "Brasilien", "Argentina", "Argentinien", "Australia",
    "Australien", "New Zealand", "Neuseeland", "Japan", "China", "India", "Indien", "Russia",
    "Russland", "Turkey", "Türkei", "Ukraine", "South Africa", "Südafrika", "Liechtenstein",
    "Croatia", "Kroatien", "Slovenia", "Slowenien", "Romania", "Rumänien", "Bulgaria", "Bulgarien",
    "Singapore", "Singapur", "Israel", "Egypt", "Ägypten", "Nigeria", "Kenya", "Kenia",
    "Schweizer", "Deutscher", "Österreicher", "Irish", "German", "Austrian", "Swiss",
    "Australian", "American", "British", "Canadian",
)
CITIES = (
    # Germany, Austria, Switzerland
    "Berlin", "Hamburg", "München", "Munich", "Köln", "Cologne", "Frankfurt", "Stuttgart",
    "Düsseldorf", "Dortmund", "Essen", "Leipzig", "Bremen", "Dresden", "Hannover", "Nürnberg",
    "Duisburg", "Bochum", "Wuppertal", "Bielefeld", "Bonn", "Münster", "Karlsruhe", "Mannheim",
    "Augsburg", "Wiesbaden", "Mainz", "Kiel", "Freiburg", "Heidelberg", "Regensburg", "Potsdam",
    "Rostock", "Erfurt", "Magdeburg", "Lübeck", "Aachen", "Ulm", "Würzburg", "Göttingen",
    "Wien", "Vienna", "Graz", "Linz", "Salzburg", "Innsbruck", "Klagenfurt", "Villach", "Wels",
    "St. Pölten", "Dornbirn", "Bregenz", "Zürich", "Zurich", "Genf", "Geneva", "Basel", "Bern",
    "Lausanne", "Winterthur", "Luzern", "Lucerne", "St. Gallen", "Lugano", "Biel", "Thun", "Zug",
    # United States, Canada
    "New York", "Los Angeles", "Chicago", "Houston", "Phoenix", "Philadelphia", "San Antonio",
    "San Diego", "Dallas", "San Jose", "Austin", "Seattle", "Denver", "Boston", "Portland",
    "Atlanta", "Miami", "Detroit", "Minneapolis", "Nashville", "Baltimore", "Pittsburgh",
    "Sacramento", "Las Vegas", "Orlando", "Tampa", "Cleveland", "Cincinnati", "Columbus",
    "Charlotte", "Raleigh", "Salt Lake City", "Boise", "Boulder", "Pasadena", "Honolulu",
    "Anchorage", "Albuquerque", "Tucson", "Omaha", "Milwaukee", "Madison", "Kansas City",
    "St. Louis", "New Orleans", "Birmingham", "Richmond", "Toronto", "Montreal", "Vancouver",
    "Calgary", "Ottawa", "Edmonton", "Winnipeg", "Halifax", "Victoria",
    # United Kingdom, Ireland
    "London", "Manchester", "Liverpool", "Leeds", "Sheffield", "Bristol", "Newcastle",
    "Newcastle upon Tyne", "Nottingham", "Leicester", "Coventry", "Bradford", "Cardiff",
    "Edinburgh", "Glasgow", "Aberdeen", "Dundee", "Belfast", "Oxford", "Cambridge", "Brighton",
    "York", "Bath", "Southampton", "Plymouth", "Dublin", "Cork", "Galway", "Limerick",
    "Waterford", "Kilkenny", "Sligo", "Drogheda",
    # Australia, New Zealand
    "Sydney", "Melbourne", "Brisbane", "Perth", "Adelaide", "Hobart", "Darwin", "Canberra",
    "Gold Coast", "Newcastle", "Geelong", "Ballarat", "Bendigo", "Cairns", "Townsville",
    "Auckland", "Wellington", "Christchurch", "Hamilton", "Tauranga", "Dunedin", "Napier",
    "Nelson", "Rotorua", "Queenstown", "Palmerston North",
    # Netherlands, France, Portugal, Brazil
    "Amsterdam", "Rotterdam", "Den Haag", "Utrecht", "Eindhoven", "Paris", "Lyon", "Marseille",
    "Toulouse", "Nice", "Lisboa", "Lisbon", "Porto", "São Paulo", "Rio de Janeiro",
)

#: Each custom recognizer: its Presidio entity, its patterns (name, regex, score),
#: context words, an optional deny list, and whether the patterns are
#: case-sensitive. Every one runs for every language.
CUSTOM_RECOGNIZERS = (
    {
        "name": "CustomStreetRecognizer", "entity": "STREET_ADDRESS",
        "case_sensitive": True,
        "patterns": [
            ("en street with number", rf"\b\d{{1,5}}[A-Za-z]?,?{_SEP}(?:[A-Z][A-Za-z'\-]+{_SEP}){{1,3}}{_EN_STREET}(?![A-Za-z])", 0.5),
            ("en street", rf"\b(?:[A-Z][A-Za-z'\-]+{_SEP}){{1,3}}{_EN_STREET}(?![A-Za-z])", 0.3),
            ("de street with number", rf"\b[A-ZÄÖÜ][\wäöüßéè\-]*{_DE_STREET}{_SEP}\d{{1,4}}[a-z]?\b", 0.5),
            ("de street", rf"\b[A-ZÄÖÜ][\wäöüßéè\-]*{_DE_STREET}\b", 0.3),
            ("de two-word street", rf"\b[A-ZÄÖÜ][\wäöüßéè\-]+{_SEP}(?:Straße|Strasse|Weg|Gasse|Allee|Platz|Ring|Damm|Ufer)(?:{_SEP}\d{{1,4}}[a-z]?)?\b", 0.4),
        ],
        "context": ["address", "street", "lives", "resides", "residing", "located", "adresse", "anschrift",
                    "wohnt", "wohne", "straße", "strasse", "deliver", "lieferadresse", "zustellung"],
    },
    {
        "name": "CustomUnitRecognizer", "entity": "BUILDING_NUMBER",
        "case_sensitive": False,
        "patterns": [
            ("unit", rf"\b(?:Apt\.?|Apartment|Suite|Unit|Building|Bldg\.?|Flat|Floor|Fl\.?|Wohnung|Whg\.?|Haus|Stock|Tür|Top|Stiege|Postfach|PO{_SEP}Box|Box){_SEP}?(?:No\.?|Nr\.?)?{_SEP}?\d{{1,5}}[A-Za-z]?\b", 0.4),
            ("house number", r"\b\d{1,4}[a-z]?\b", 0.01),
        ],
        "context": ["address", "street", "house", "number", "building", "hausnummer", "adresse", "straße",
                    "strasse", "wohnung", "apartment", "suite"],
    },
    {
        "name": "CustomZipRecognizer", "entity": "ZIP_CODE",
        "case_sensitive": True,
        "patterns": [
            ("us zip", r"\b\d{5}(?:-\d{4})?\b", 0.1),
            ("four digits", r"\b\d{4}\b", 0.01),
            ("uk postcode", rf"\b[A-Z]{{1,2}}\d[A-Z\d]?{_SEP}?\d[A-Z]{{2}}\b", 0.4),
            ("ca postcode", rf"\b[A-Z]\d[A-Z]{_SEP}?\d[A-Z]\d\b", 0.4),
            ("ie eircode", rf"\b[A-Z]\d{{2}}{_SEP}?[A-Z0-9]{{4}}\b", 0.3),
            ("zip before city", r"\b\d{4,5}(?=[   ][A-ZÄÖÜ][a-zäöüß]{2,})", 0.3),
        ],
        "context": ["zip", "postcode", "postal", "plz", "postleitzahl", "code", "address", "adresse"],
    },
    {
        "name": "CustomStateRecognizer", "entity": "STATE",
        "case_sensitive": True,
        "patterns": [
            ("us state code before zip", r"\b(?:" + "|".join(US_STATE_CODES) + r")(?=[   ]+\d{5})", 0.6),
            ("au state code", r"\b(?:NSW|VIC|QLD|WA|SA|TAS|NT|ACT)\b", 0.3),
            ("county", r"\b(?:County|Co\.)[   ][A-Z][a-z]+\b", 0.5),
            ("kanton", r"\b(?:Kanton|Canton|Bundesland|state of|State of)[   ][A-ZÄÖÜ][\wäöü\-]+\b", 0.4),
        ],
        "deny_list": list(US_STATES) + list(OTHER_REGIONS),
        "context": ["state", "county", "province", "region", "bundesland", "kanton", "land"],
    },
    {
        "name": "CustomCountryRecognizer", "entity": "COUNTRY",
        "case_sensitive": True, "patterns": [], "deny_list": list(COUNTRIES),
        "context": ["country", "land", "staat", "nationality", "staatsbürgerschaft", "citizen", "moved"],
    },
    {
        "name": "CustomCityRecognizer", "entity": "CITY",
        "case_sensitive": True,
        "patterns": [
            ("city after zip", r"(?<=\b\d{4,5}[   ])[A-ZÄÖÜ][a-zäöüß]+(?:[   \-][A-ZÄÖÜa-z][a-zäöüß]+){0,2}\b", 0.4),
            ("city before state", r"\b[A-Z][a-z]+(?:[   ][A-Z][a-z]+)?(?=,[   ]*(?:[A-Z]{2}[   ]\d{5}|(?:NSW|VIC|QLD|WA|SA|TAS|NT|ACT)\b))", 0.4),
        ],
        "deny_list": list(CITIES),
        "context": ["city", "town", "stadt", "ort", "wohnort", "in", "from", "aus"],
    },
    {
        "name": "CustomDateOfBirthRecognizer", "entity": "DATE_OF_BIRTH",
        "case_sensitive": False,
        "patterns": [
            ("numeric", r"\b\d{1,2}[./-]\d{1,2}[./-](?:19|20)\d{2}\b", 0.3),
            ("iso", r"\b(?:19|20)\d{2}-\d{2}-\d{2}\b", 0.3),
            ("day month year", rf"\b\d{{1,2}}(?:\.|st|nd|rd|th)?(?:{_SEP}of)?{_SEP}{_MONTHS},?{_SEP}(?:19|20)\d{{2}}\b", 0.4),
            ("month day year", rf"\b{_MONTHS}{_SEP}\d{{1,2}}(?:st|nd|rd|th)?,?{_SEP}(?:19|20)\d{{2}}\b", 0.4),
        ],
        "context": ["born", "birth", "dob", "birthday", "geboren", "geburtstag", "geburtsdatum",
                    "date", "datum", "age"],
    },
    {
        "name": "CustomAgeRecognizer", "entity": "AGE",
        "case_sensitive": False,
        # Presidio Research's notebook 5 AgeRecognizer pattern, with German context words.
        "patterns": [("age (very weak)", r"\b(110|[1-9]?[0-9])\b", 0.01)],
        "context": ["month", "old", "turn", "age", "aged", "y/o", "years", "alt", "alter", "jahre",
                    "jährig", "bin"],
    },
    {
        "name": "CustomPhoneRecognizer", "entity": "PHONE_NUMBER",
        "case_sensitive": False,
        "patterns": [
            ("international", r"(?<![\w+])\+\d{1,3}(?:[   .\-]?\(?\d{1,5}\)?){2,5}(?!\w)", 0.5),
            ("national", r"(?<![\w+])(?:\(\d{2,5}\)[   .\-]?|\d{2,5}[   .\-/])\d{2,8}(?:[   .\-]\d{2,8}){0,3}(?!\w)", 0.2),
        ],
        "context": ["phone", "tel", "telephone", "mobile", "cell", "call", "fax", "contact", "telefon",
                    "handy", "mobil", "rufnummer", "telefonnummer", "unter", "erreichbar", "nummer"],
    },
    {
        "name": "CustomIbanRecognizer", "entity": "IBAN_CODE",
        "case_sensitive": True,
        # No checksum: Presidio's IbanRecognizer drops mistyped IBANs.
        "patterns": [("iban shape", r"\b[A-Z]{2}\d{2}(?:[   \-]?[A-Z0-9]{2,4}){3,8}\b", 0.5)],
        "context": ["iban", "account", "konto", "bank", "rekening", "compte"],
    },
    {
        "name": "CustomCardRecognizer", "entity": "CREDIT_CARD",
        "case_sensitive": False,
        # No Luhn check, and a four-digit card tail after a card cue.
        "patterns": [
            ("card shape", r"\b\d{4}(?:[   \-]?\d{4}){2}[   \-]?\d{1,4}\b", 0.4),
            ("card tail", r"(?<=\b(?:ending(?: in| with)?|ends(?: in| with)|endet(?: auf| mit)|endend auf)[   ])\d{4}\b", 0.5),
        ],
        "context": ["card", "credit", "visa", "mastercard", "karte", "kreditkarte", "kartennummer"],
    },
    {
        "name": "CustomSsnRecognizer", "entity": "SSN",
        "case_sensitive": False,
        "patterns": [
            ("us ssn shape", r"\b\d{3}[\- ]\d{2}[\- ]\d{4}\b", 0.3),
            ("ch ahv", r"\b756[.\-]?\d{4}[.\-]?\d{4}[.\-]?\d{2}\b", 0.5),
        ],
        "context": ["ssn", "social", "security", "sozialversicherung", "sozialversicherungsnummer",
                    "versicherungsnummer", "ahv", "insurance"],
    },
    {
        "name": "CustomSteuerIdRecognizer", "entity": "TAX_ID",
        "case_sensitive": False,
        "patterns": [
            ("steuer-id spaced", r"\b\d{2}[   ]?\d{3}[   ]?\d{3}[   ]?\d{3}\b", 0.3),
            ("vat", r"\b(?:ATU\d{8}|DE\d{9}|CHE[\-.]?\d{3}[.\-]?\d{3}[.\-]?\d{3})\b", 0.5),
        ],
        "context": ["steuer", "steuer-id", "steuernummer", "steueridentifikationsnummer", "tax", "tin",
                    "vat", "ust", "taxidentnr", "identifikationsnummer"],
    },
    {
        "name": "CustomIdNumberRecognizer", "entity": "ID_NUMBER",
        "case_sensitive": False,
        "patterns": [
            ("id with letters", r"\b(?=[A-Z0-9.\-]*\d{4})[A-Z]{1,4}[\-. ]?\d[0-9A-Z.\-]{3,16}\b", 0.05),
            ("digit run", r"\b\d{6,13}[A-Z]?\b", 0.05),
            ("grouped digits", r"\b\d{2,4}(?:[.\- ]\d{2,7}){1,3}[A-Z]?\b", 0.02),
        ],
        "context": ["id", "nummer", "number", "no", "nr", "passport", "pass", "reisepass", "ausweis",
                    "personalausweis", "identity", "card", "licence", "license", "führerschein",
                    "fahrerlaubnis", "national", "tax", "steuer", "document", "identification",
                    "identitätskarte", "passnummer"],
    },
    {
        "name": "CustomLicensePlateRecognizer", "entity": "LICENSE_PLATE",
        "case_sensitive": True,
        "patterns": [
            ("de at ch plate", r"\b[A-ZÄÖÜ]{1,3}[\-  ]?[A-Z]{0,2}[\-  ]?\d{1,6}[A-Z]{0,2}\b", 0.05),
            ("uk plate", r"\b[A-Z]{2}\d{2}[  ]?[A-Z]{3}\b", 0.3),
            ("ie plate", r"\b\d{2,3}-[A-Z]{1,2}-\d{1,6}\b", 0.4),
            ("us plate", r"\b[A-Z0-9]{2,4}[\- ]?[A-Z0-9]{3,4}\b", 0.01),
        ],
        "context": ["plate", "license plate", "licence plate", "registration", "kennzeichen",
                    "nummernschild", "car", "vehicle", "fahrzeug", "auto"],
    },
    {
        "name": "CustomUsernameRecognizer", "entity": "USERNAME",
        "case_sensitive": False,
        "patterns": [(
            "after a username cue",
            r"(?<=\b(?:username|user name|user|login|handle|benutzername|benutzer|nutzername|account)\b[   ]*(?:is|lautet|ist|:|=)?[   ]*[\"'@]?)(?!(?:is|lautet|ist)\b)[A-Za-z][\w.\-]{2,40}",
            0.3,
        )],
        "context": ["username", "user", "login", "handle", "benutzername"],
    },
    {
        "name": "CustomNhsRecognizer", "entity": "UK_NHS_LOOSE",
        "case_sensitive": False,
        # No checksum: UkNhsRecognizer rejects a mistyped NHS number.
        "patterns": [("nhs shape", r"\b\d{3}[   \-]?\d{3}[   \-]?\d{4}\b", 0.3)],
        "context": ["nhs", "health", "patient"],
    },
    {
        "name": "CustomCpfRecognizer", "entity": "CPF",
        "case_sensitive": False,
        "patterns": [("cpf", r"\b\d{3}\.?\d{3}\.?\d{3}-?\d{2}\b", 0.4)],
        "context": ["cpf", "documento", "cadastro"],
    },
    {
        "name": "CustomBsnRecognizer", "entity": "BSN",
        "case_sensitive": False,
        "patterns": [("nine digits", r"\b\d{9}\b", 0.1)],
        "context": ["bsn", "burgerservicenummer", "sofinummer"],
    },
    {
        "name": "CustomCompanyRecognizer", "entity": "ORGANIZATION",
        "case_sensitive": True,
        "patterns": [(
            "legal form",
            r"\b(?:[A-ZÄÖÜ][\w&'\-]*[  ]){0,3}[A-ZÄÖÜ][\w&'\-]*[  ](?:GmbH|AG|KG|OHG|SE|UG|e\.V\.|Inc\.?|LLC|Ltd\.?|Limited|Corp\.?|Co\.|plc|PLC|S\.A\.|B\.V\.|N\.V\.|Pty(?: Ltd)?)(?![\w])",
            0.5,
        )],
        "context": ["company", "firm", "employer", "works", "firma", "unternehmen", "arbeitet", "bei"],
    },
    {
        "name": "CustomTitledNameRecognizer", "entity": "PERSON",
        "case_sensitive": True,
        "patterns": [
            ("after a title", r"(?<=\b(?:Mr|Mrs|Ms|Miss|Dr|Prof|Herr|Frau|Herrn)\.?[   ](?:Dr\.[   ])?)[A-ZÄÖÜ][a-zäöüß'\-]+(?:[   ][A-ZÄÖÜ][a-zäöüß'\-]+)?", 0.4),
            ("after a greeting", r"(?<=\b(?:Hi|Hello|Dear|Hey|Hallo|Liebe|Lieber|Guten Tag|From:|Von:|signed for by|name is|Name ist|heiße)[   ,]+)[A-ZÄÖÜ][a-zäöüß'\-]+(?:[   ][A-ZÄÖÜ][a-zäöüß'\-]+)?", 0.3),
            ("full name field", r"(?<=(?:fullName|full_name|name|account_holder)[\"']?[  ]*[:=][  ]*[\"']?)[A-ZÄÖÜ][a-zäöüß'\-]+(?:[  ][A-ZÄÖÜ][a-zäöüß'\-]+)?", 0.4),
        ],
        "context": ["name", "mr", "mrs", "ms", "dr", "herr", "frau"],
    },
)

#: Presidio's own PhoneRecognizer with the corpus's regions and the most lenient
#: phonenumbers matching, as a second, separately switchable unit.
PHONE_WIDE = {
    "name": "PhoneRecognizerWide",
    "regions": ["US", "GB", "DE", "AT", "CH", "FR", "NL", "BE", "IE", "AU", "NZ", "CA", "BR", "PT",
                "IT", "ES", "IN", "IL"],
    "leniency": 0,
}

#: Coordinate descent: each round tries every single-unit move in a fixed order
#: (NER backbone, extra NER scopes, pattern scopes, custom recognizers, context
#: mode, thresholds, allow list) and keeps a move only when it strictly improves
#: the objective. It stops after a round without an improvement or at `max_rounds`.
SEARCH = {"max_rounds": 8}

#: Starting points. `presidio-default` is the comparison's presidio-all row
#: (spaCy NER, default pattern recognizers plus the nine German ones, default
#: context, threshold 0). `everything` switches every unit on at its floor.
STARTS = ("presidio-default", "everything")

#: The German recognizers the comparison's presidio-all row adds (compare.GERMAN_RECOGNIZERS).
PRESIDIO_ALL_GERMAN = (
    "DeTaxIdRecognizer", "DeTaxNumberRecognizer", "DePassportRecognizer", "DeIdCardRecognizer",
    "DeSocialSecurityRecognizer", "DeHealthInsuranceRecognizer", "DeKfzRecognizer",
    "DeHandelsregisterRecognizer", "DePlzRecognizer",
)

#: Canonical gold labels for every entity the pool can emit that neither the
#: comparison's `presidio` table (../label-map.json) nor Presidio Research's
#: `extra_labels` (../theirbench/vendor-tuned.json) names. Labels drive only the
#: typed metrics, the common-intersection view and v3's repeat credit, never
#: leaked or false-positive bytes. The vocabulary follows those two tables: a
#: layer A-only identifier (BSN, CPF, NHS) maps as Gaze's own `custom:bsn`,
#: `custom:cpf` and `custom:nhs_number` do.
EXTRA_LABELS = {
    "ADDRESS": ["BUILDINGNUM", "CITY", "COUNTRY", "REGION", "STATE", "STREET", "ZIP"],
    "BUILDING_NUMBER": ["BUILDINGNUM"],
    "BANK_ACCOUNT": ["IBAN"],
    "ID_NUMBER": ["IDCARDNUM", "PASSPORTID", "DRIVERLICENSENUM", "NATIONALID", "TAXNUM"],
    "ID_CARD": ["IDCARDNUM", "NATIONALID"],
    "NATIONAL_ID": ["NATIONALID", "IDCARDNUM"],
    "PASSPORT": ["PASSPORTID"],
    "UK_PASSPORT": ["PASSPORTID"],
    "ES_PASSPORT": ["PASSPORTID"],
    "KR_PASSPORT": ["PASSPORTID"],
    "DRIVER_LICENSE": ["DRIVERLICENSENUM"],
    "DE_FUEHRERSCHEIN": ["DRIVERLICENSENUM"],
    "UK_DRIVING_LICENCE": ["DRIVERLICENSENUM"],
    "KR_DRIVER_LICENSE": ["DRIVERLICENSENUM"],
    "SSN": ["SSN"],
    "TAX_ID": ["TAXNUM"],
    "DE_VAT_ID": ["TAXNUM"],
    "PH_TIN": ["TAXNUM"],
    "CPF": ["NATIONALID", "TAXNUM"],
    "CNPJ": ["NATIONALID", "TAXNUM"],
    "BSN": ["NATIONALID"],
    "CA_SIN": ["NATIONALID"],
    "FI_PERSONAL_IDENTITY_CODE": ["NATIONALID"],
    "KR_RRN": ["NATIONALID"],
    "KR_FRN": ["NATIONALID"],
    "NG_NIN": ["NATIONALID"],
    "PH_UMID": ["NATIONALID"],
    "SE_PERSONNUMMER": ["NATIONALID"],
    "TH_TNIN": ["NATIONALID"],
    "TR_NATIONAL_ID": ["NATIONALID"],
    "ZA_ID_NUMBER": ["NATIONALID"],
    "UK_POSTCODE": ["ZIP"],
    "UK_VEHICLE_REGISTRATION": ["LICENSEPLATENUM"],
    "NG_VEHICLE_REGISTRATION": ["LICENSEPLATENUM"],
    "TR_LICENSE_PLATE": ["LICENSEPLATENUM"],
    "USERNAME": ["USERNAME"],
    "UK_NHS_LOOSE": [],
    "ABA_ROUTING_NUMBER": [], "BIRTH_CERTIFICATE": [], "CARD_BRAND": [], "CARD_EXPIRY": [],
    "DE_BSNR": [], "DE_LANR": [], "DIGITAL_SIGNATURE": [], "FLIGHT_NUMBER": [],
    "HEALTH_INSURANCE_ID": [], "IN_GSTIN": [], "INSURANCE_NUMBER": [], "KR_BRN": [],
    "MEDICAL_CONDITION": [], "MEDICATION": [], "PASSPORT_EXPIRY": [], "REGISTRATION_NUMBER": [],
    "RESERVATION_NUMBER": [], "SERIAL_NUMBER": [], "SE_ORGANISATIONSNUMMER": [], "SG_UEN": [],
    "STUDENT_ID": [], "TICKET_NUMBER": [], "TRANSACTION_NUMBER": [], "US_MBI": [], "US_NPI": [],
    "VISA_NUMBER": [],
}
