"""CC0 invented full-name case coverage with independently labelled benign phrases.

Gold is the producer's given-name/surname components, including every repeat.
Seed names describe input contracts, never assert a detector actually caught them.
"""

NAMES = {
    "dev": {
        "en": (("Nella", "Frostwillow"), ("Orin", "Lee", "Mosswharf")),
        "de": (("Selina", "Wolkenhain"), ("Jorin", "Lotte", "Dämmerquell")),
        "fr": (("Éliane", "Brumelande"), ("Rémi", "Anne", "Clairfeuille")),
    },
    "test": {
        "en": (("Elara", "Wrenhollow"), ("Tovin", "Mae", "Thistlemere")),
        "de": (("Alina", "Wolkenfeld"), ("Torina", "Lotta", "Falkenwiese")),
        "fr": (("Maëlle", "Roselune"), ("Lucien", "Albe", "Ventofleur")),
    },
}
CASES = {"lower": str.lower, "upper": str.upper, "title": lambda value: value}
SURFACES = ("chat", "tool_json", "form", "greeting")
REGIONS = {"en": "US", "de": "DE", "fr": "FR"}
# Partition-specific producer text; shared identifier words are field contracts.
TEMPLATES = {
    "dev": {
        "en": ("user: contact {V}, please.", '{"name":"send_note","arguments":"{\\"recipient_name\\":\\"{V}\\"}"}',
               "Full name: {V}\nPurpose: review", "Dear {V},\nThank you for the note."),
        "de": ("user: kontaktiere {V}, bitte.", '{"name":"schreibe_notiz","arguments":"{\\"voller_name\\":\\"{V}\\"}"}',
               "Voller Name: {V}\nZweck: Prüfung", "Hallo {V},\nDanke für die Nachricht."),
        "fr": ("user: contactez {V}, merci.", '{"name":"envoyer_note","arguments":"{\\"nom_complet\\":\\"{V}\\"}"}',
               "Nom complet: {V}\nObjet: examen", "Bonjour {V},\nMerci pour la note."),
    },
    "test": {
        "en": ("user: reply to {V}, tomorrow.", '{"name":"draft_reply","arguments":"{\\"person_name\\":\\"{V}\\"}"}',
               "Person name = {V}\nRequest = reply", "Dear {V},\nPlease confirm receipt."),
        "de": ("user: antworte {V}, morgen.", '{"name":"antwort_entwurf","arguments":"{\\"personenname\\":\\"{V}\\"}"}',
               "Name der Person = {V}\nAnfrage = Antwort", "Guten Tag {V},\nBitte bestätigen Sie den Empfang."),
        "fr": ("user: répondez à {V}, demain.", '{"name":"preparer_reponse","arguments":"{\\"nom_personne\\":\\"{V}\\"}"}',
               "Nom de la personne = {V}\nDemande = réponse", "Cher {V},\nVeuillez confirmer la réception."),
    },
}
BENIGN = {
    "dev": {"en": ("service desk", "support review team"), "de": ("technischer dienst", "die zuständige abteilung"),
            "fr": ("service technique", "notre équipe technique")},
    "test": {"en": ("technical support", "the support team"), "de": ("technischer support", "das zuständige team"),
             "fr": ("support technique", "notre équipe assistance")},
}
# Common words with a surname reading: the producer explicitly means plants,
# colours or objects here. These are benign even after an identical contact cue.
NOUNS = {"dev": {"en": "alder", "de": "fichte", "fr": "rose"},
         "test": {"en": "hazel", "de": "buche", "fr": "blanc"}}


def records(api, partition):
    result = []

    def emit(layer, language, parts, case, surface, mode, template, index, benign=False):
        fields = {}
        markers = []
        for position, part in enumerate(parts):
            marker = f"N{position}"
            markers.append("{" + marker + "}")
            label = "SURNAME" if position == len(parts) - 1 else "FIRSTNAME"
            fields[marker] = (CASES[case](part), "decoy:service_phrase" if benign else label)
        rendered = template.replace("{V}", " ".join(markers))
        if mode != "first":
            seed = []
            for position, part in enumerate(parts):
                marker = f"S{position}"
                seed.append("{" + marker + "}")
                label = "SURNAME" if position == len(parts) - 1 else "FIRSTNAME"
                fields[marker] = (part, "decoy:service_phrase" if benign else label)
            prefix = "reply to " + " ".join(seed) + ", please.\n" if mode == "rule_seed" else " ".join(seed) + "\n"
            rendered = prefix + rendered
        text, gold, decoys = api._fill_with_decoys(rendered, fields)
        result.append(api.Record(
            uid=f"agentic-{partition}-{layer}-full-name-{language}-{index}-{case}-{surface}-{mode}",
            partition=partition, layer=layer, family="full_name_case",
            surface=f"fullnames_{language}_{len(parts)}part_{case}_{surface}_{mode}",
            validity=api.BENIGN if benign else api.UNCHECKED,
            group=f"{partition}-full-name-{language}-{index}",
            template=f"full-name-case/{partition}/{language}/{surface}/{mode}",
            language=language, region=REGIONS[language], text=text, gold=gold, decoys=decoys,
        ))

    for language, names in NAMES[partition].items():
        for index, parts in enumerate(names):
            for case in CASES:
                for surface, template in zip(SURFACES, TEMPLATES[partition][language], strict=True):
                    emit("A", language, parts, case, surface, "first", template, index)
                    for mode in ("rule_seed", "ner_seed"):
                        emit("R", language, parts, case, surface, mode, template, index)
                    benign_parts = tuple(BENIGN[partition][language][index].split())
                    for mode in ("first", "rule_seed", "ner_seed"):
                        emit("D", language, benign_parts, case, surface, mode, template, index, True)
        # Unambiguously generic salutations, including the user-reported cues.
        phrases = (("contact technical support", "reply to the team", "dear customer", "the hazel is a shrub")
                   if language == "en" else
                   ("kontaktiere den support", "antworte dem team", "lieber kunde", "die buche ist ein baum")
                   if language == "de" else
                   ("contactez le support", "répondez à l'équipe", "cher client", "le blanc est une couleur"))
        if partition == "dev":
            phrases = tuple(phrase + "; public template" for phrase in phrases)
        for index, phrase in enumerate(phrases):
            for case in CASES:
                emit("D", language, (phrase,), case, "benign_phrase", "first", "{V}", 10 + index, True)
        for case in CASES:
            emit("D", language, (NOUNS[partition][language],), case, "noun_field", "first",
                 'botanical_or_colour_term="{V}"', 20, True)
    return result
