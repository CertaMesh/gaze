"""CC0 chat utterances. The speaker assigns the referent, not a detector or lexicon."""

SURFACE_PREFIX = "chat_names_"

# Invented names, a name-word, and an established given/surname pair from the
# partition's synthetic people. Each record stands alone without a name seed.
CAST = {
    ("dev", "en"): ("Velna", "Fernwharf", "Summer", "Oliver", "Walker"),
    ("test", "en"): ("Emrel", "Mistcroft", "Amber", "Olivia", "Carter"),
    ("dev", "de"): ("Nelvin", "Birkensaum", "Ernst", "Lena", "Meyer"),
    ("test", "de"): ("Selka", "Dünenwacht", "Rosa", "Lukas", "Schmidt"),
}

# Mentions here address automated services; personal handles are out of scope.
PROMPTS = {
    ("dev", "en"): {
        "intro": ("Hello, my name is {V}.", "Quick introduction: I'm {V}.", "Hi, this is {V} speaking."),
        "ask": ("Please ask {V} about lunch.",),
        "cc": ("Can you cc {V} on the agenda?",),
        "after_handle": ("One more guest for your list, @guestlistbot: {V}.", "@guestlistbot {V} is joining us."),
    },
    ("test", "en"): {
        "intro": ("For introductions, my name is {V}.", "Hi! I’m {V}, your new colleague.", "This is {V} from the night shift."),
        "ask": ("If you're unsure, ask {V} first.",),
        "cc": ("When you send the minutes, cc {V}.",),
        "after_handle": ("The speaker for tomorrow, @rosterbot: {V}.", "@rosterbot {V} will cover the desk."),
    },
    ("dev", "de"): {
        "intro": ("Hallo, ich heiße {V}.", "Kurze Vorstellung: Ich bin {V}.", "Guten Tag, hier ist {V}."),
        "ask": ("Bitte frag {V} nach dem Termin.",),
        "cc": ("Beim Versand bitte cc {V}.",),
        "after_handle": ("Noch ein Gast für die Liste, @gaestebot: {V}.", "@gaestebot {V} kommt zum Essen."),
    },
    ("test", "de"): {
        "intro": ("Zur Vorstellung: Mein Name ist {V}.", "Guten Abend! Ich bin {V}, die neue Aushilfe.", "Hier ist {V} aus der Spätschicht."),
        "ask": ("Wenn du Hilfe brauchst, frag {V}.",),
        "cc": ("Für die Antwort gilt: cc {V} nicht vergessen.",),
        "after_handle": ("Die Vertretung von morgen, @planungsbot: {V}.", "@planungsbot {V} übernimmt die Frühschicht."),
    },
}

# Benign uses of the same words and cues, not personal aliases relabelled D.
ORDINARY = {
    ("dev", "en"): {
        "intro": ("Hello, my name is printed on the badge already.", "I'm summer staff at the pool.", "Hi, this is just a draft."),
        "ask": ("Please ask summer staff about opening hours.", "Please ask which train stops here."),
        "cc": ("Can you cc the shared service queue?", "Also cc grant applications to the shared archive."),
        "after_handle": ("One more task for you, @guestlistbot: export the list.", "@guestlistbot mark completed items."),
    },
    ("test", "en"): {
        "intro": ("My name is missing from the spreadsheet.", "I’m amber on the status board, not green.", "This is fine for a prototype."),
        "ask": ("At the trade fair, ask amber suppliers for samples.", "Before leaving, ask again."),
        "cc": ("When you send the minutes, cc technical support.", "Please cc bill reminders to the accounts queue."),
        "after_handle": ("Change of schedule, @rosterbot: postpone the meeting.", "@rosterbot grant read access to the rota."),
    },
    ("dev", "de"): {
        "intro": ("Ich heiße diesen Vorschlag gut.", "Ich bin ernst geblieben und habe nicht gelacht.", "Hier ist genug Platz für alle."),
        "ask": ("Bitte frag mal nach dem Beginn.", "Ich frag im Ernst, ist das nötig?"),
        "cc": ("Beim Versand bitte cc an die Sammeladresse.", "Die Abkürzung cc ist auf dem Vordruck erklärt."),
        "after_handle": ("Eine Bitte, @gaestebot: zähle die freien Plätze.", "@gaestebot ernst gemeinte Anfragen bitte getrennt zählen."),
    },
    ("test", "de"): {
        "intro": ("Mein Name ist dort nicht eingetragen.", "Ich bin rosa geschminkt und trage Glitzer.", "Hier ist noch die Ersatzliste."),
        "ask": ("Im Stoffladen frag nach rosa Leinen.", "Frag sie doch nach der Startzeit."),
        "cc": ("Für die Antwort gilt: cc an das Funktionspostfach.", "Im Handbuch werden cc und bcc unterschieden."),
        "after_handle": ("Neue Aufgabe, @planungsbot: prüfe die Auslastung.", "@planungsbot rosa markierte Schichten bitte ausblenden."),
    },
}


def records(api, partition):
    result = []
    for language in ("en", "de"):
        key = partition, language
        given, surname, word, common_given, common_surname = CAST[key]

        def emit(layer, cue, index, shape, template, fields):
            text, gold = api._fill(template, fields)
            result.append(api.Record(
                uid=f"agentic-{partition}-{layer}-chat-name-{language}-{cue}-{index}-{shape}",
                partition=partition, layer=layer, family="chat_name", language=language,
                region={"en": "US", "de": "DE"}[language], text=text, gold=gold,
                validity=api.BENIGN if layer == "D" else api.UNCHECKED,
                surface=f"{SURFACE_PREFIX}{language}_{cue}_{shape}",
                group=f"{partition}-chat-name-{language}",
                template=f"chat-name/{partition}/{language}/{layer}/{cue}/{index}",
            ))

        for cue, templates in PROMPTS[key].items():
            for index, template in enumerate(templates):
                for shape, first, last in (
                    ("single_lower", given.lower(), None), ("single_title", given, None),
                    ("word_lower", word.lower(), None), ("word_title", word, None),
                    ("pair_lower", given.lower(), surname.lower()), ("pair_mixed", given.lower(), surname),
                    ("established_single_lower", common_given.lower(), None),
                    ("established_pair_lower", common_given.lower(), common_surname.lower()),
                    ("established_pair_mixed", common_given, common_surname.lower()),
                ):
                    fields = {"V": (first, "GIVENNAME")}
                    rendered = template
                    if last is not None:
                        fields["S"] = last, "SURNAME"
                        rendered = template.replace("{V}", "{V} {S}")
                    emit("A", cue, index, shape, rendered, fields)
        for cue, templates in ORDINARY[key].items():
            for index, template in enumerate(templates):
                emit("D", cue, index, "ordinary", template, {})
    return result
