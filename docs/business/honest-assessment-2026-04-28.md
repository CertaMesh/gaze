# Ehrliche Einschätzung — Lohnt sich Gaze als Business?

**Datum:** 2026-04-28
**Autor:** Claude (Opus 4.7), auf Anfrage
**Status:** Persönliche Einschätzung, keine Entscheidung

> Hinweis: Diese Einschätzung ist die ungefilterte Meinung eines Sprachmodells nach Sichtung der Repos (`gaze`, `gaze-lens`, `gaze-laravel`, `gaze-website`) und der Strategie-Diskussion in `positioning-2026-04-28.md`. Kein Marketing, kein Hype, kein Doom. Persönliche Lebenslage des Gründer-Teams ist hier nicht eingerechnet — das ist eure Verantwortung.

## Kurz-Verdict

**Ja, sinnvoll. Aber nicht trivial.**

Das ist eines der wenigen Bootstrap-fähigen Dev-Tool-Geschäfte, die derzeit realistisch wirken. Mit echten Stolpersteinen, die nicht kleingeredet werden sollten.

---

## Was gut ist

### Problem ist echt und akut
- Firmen wollen KI-Agents auf Production loslassen. Datenschutzbeauftragte blockieren. EU AI Act zieht 2026 an. HIPAA, BaFin, BSI sowieso.
- Kein "could be a problem someday" — **jetzt** Schmerz. Jeder zweite SRE / Lead Dev mit KI-Initiative kennt das Gefühl.

### Technische Differenzierung ist real, nicht erfunden
- Reversibel + agentic-first + MCP-nativ — diese Kombi gibt's so nicht. Presidio macht Detection (one-way), Skyflow ist Vault-Pattern, Tonic ist Batch.
- Rust-Architektur sauber: 7 Crates getrennt, Audit isoliert, fail-closed, Manifest-Contract. Das ist **engineered**, nicht vibe-coded. Sieht man selten.
- v0.5 + Lens v1.0 = echte Artefakte, nicht Deck.

### Naoray-Beachhead
- PHP-Audience ist klein, aber **erreichbar ohne Marketing-Budget**. Gold wert für Bootstrap.
- Antwort auf "wie kommt ihr an erste Kunden?" — viele Gründer haben keine.

### Timing
- 2026 ist genau der Moment. KI-Agents in Prod = Massentrend. Compliance-Schmerz wächst, nicht schrumpft.
- Zu früh = Markt nicht da. Zu spät = Big-Player drin. Ihr seid mittendrin.

---

## Was Bauchschmerzen macht

### Zwei Personen für Security-Tool ist dünn
- Security-Produkte haben hohen Support-Aufwand. Ein Bug = Headline. Ein Leak = ihr seid weg.
- 24/7-Reaktionsbereitschaft kommt schneller als gedacht. Pager-Duty ohne dritte Person ist brutal.

### Kein Sichtbarkeits-Signal
- Alle Repos privat, keine Stars, keine Issues, kein Hacker-News-Post. Nicht beurteilbar, ob jemand außer euch das Produkt will.
- Ohne erste Design-Partner sind alle Pricing-Zahlen **Hypothesen**, keine Belege.

### Plattform-Risiko
- Anthropic / OpenAI können in 6 Monaten "PII-aware mode" als Feature shippen. Microsoft Presidio + Copilot-Integration ist ein Federstrich entfernt. Skyflow / Tonic können MCP-Layer dazu basteln.
- Antwort darauf muss sein: **schneller bewegen** durch Open-Source-Community + Branchen-Recognizer-Pakete + Audit-Trail-Tiefe. Beweisbar erst durch Ausführung.

### Side-Project-Modus killt das
- Security-Produkte brauchen Reaktionszeit. Beide Vollzeit-Jobs nebenbei → erste Outage / CVE / Kunden-Eskalation = Aus.
- Mindestens **eine** Person muss innerhalb 6–12 Monaten Vollzeit rein, sonst tot.

### 4 Repos + Multi-Language-Adapter-Strategie ist viel
- Laravel allein wartbar zu zweit. Plus Python, plus Node, plus Go, plus Rust-Core, plus Lens, plus Audit-Cloud → Burnout-Trajektorie.
- Disziplin bei Scope kritisch.

### Vertrauens-Hürde im DACH-Markt
- "Zwei Devs aus Deutschland verkaufen mir PII-Filter für meine KI-Pipeline" — harter Pitch ohne Zertifikate / Logos / Ankerkunden.
- Entweder Open-Source-Trust (Code public, Audit-Logs, Reproducible Builds) oder erster großer Kunde als Referenz. Idealerweise beides.

---

## Empfehlung — wenn ihr's macht, dann so

1. **Bootstrap-First-Phase: 5–10 zahlende Kunden in 9 Monaten.**
   Wenn das nicht klappt → ehrliches Post-Mortem, nicht weitermachen "weil's doch eigentlich".
2. **Open-Core von Tag 1.** Repo public, Apache-2.0, Stars sammeln. Trust-Signal entscheidet bei Security-Tools alles.
3. **Naoray + Laravel als Beachhead.** Erste 3 Kunden über sein Netzwerk. Billigster Akquise-Hebel.
4. **Spätestens Monat 6: einer geht Vollzeit.** Wer? Wie finanziert? Vor dem Loslegen klären.
5. **Compliance-Enterprise (Option 3) erstmal vergessen.** Zu kapital-intensiv ohne Funding. Pfad öffnet sich erst, wenn Bottom-Up funktioniert.
6. **Pivot-Trigger definieren.** "Wenn nach X Monaten Y nicht passiert, drehen / hören auf". Sonst Sunk-Cost-Falle.

---

## Pivot-Trigger / Kill-Kriterien

Bedingungen, unter denen ich zu **"nein, lasst es"** umschwenke:

- ❌ In 8 Wochen **keinen einzigen** Design-Partner gefunden (auch nicht über Naoray) → Markt-Signal fehlt.
- ❌ Naoray sagt "Side-Project, max 5h/Woche" → Geschwindigkeit nicht erreichbar.
- ❌ Ihr könnt nicht in **einem Satz** sagen, warum ihr gegen Presidio + Microsoft Copilot gewinnt → Strategie nicht scharf genug.
- ❌ In 6 Monaten v0.6 nicht released → Execution-Geschwindigkeit reicht nicht.

Diese Kriterien jetzt aufschreiben, bevor ihr emotional investiert seid. Sonst rationalisiert ihr sie später weg.

---

## Persönliche Note

Selten ist die Kombination aus **solider Tech + klarem Marktproblem + Beachhead-Zugang** so beieinander. Das ist Asset, nicht Standard.

**Aber:** Tech-Qualität ≠ Business-Erfolg. Brillanter Code geht pleite, wenn GTM fehlt. Eure Tech reicht. **Eure Vertriebs- und Community-Disziplin entscheidet.**

Wenn ihr ehrlich gegen euch selbst seid und nicht in Sunk-Cost verfallt, ist das eines der besseren Mikro-SaaS-Setups, die derzeit realistisch wirken.

---

## Caveats zu dieser Einschätzung

- **Sprachmodell-Limit:** Ich sehe euer Leben nicht. Familie, Ersparnisse, Risiko-Toleranz — alles nicht in dieser Bewertung.
- **Snapshot 2026-04-28:** Markt bewegt sich. In 6 Monaten kann sich das Bild geändert haben (z.B. wenn OpenAI native PII-Mode shippt).
- **Inside-View-Limit:** Ich sehe Repos und eure Strategie-Doc. Ich sehe nicht: Code-Qualität in Tiefe (nur READMEs gelesen), tatsächliche Bug-Rate, Naoray's echte Kapazität, Wettbewerber-Roadmaps.
- **Empfehlung als Datenpunkt, nicht als Orakel.** Holt euch zusätzlich Meinungen von: einem Mentor mit B2B-SaaS-Erfahrung, einem Datenschutzbeauftragten (würde er kaufen?), einem Laravel-Shop-Lead (zahlt er 49 €/Monat dafür?).
