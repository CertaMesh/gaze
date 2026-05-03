# Flagschiff-Entscheidung & Reference-App-Strategie

**Datum:** 2026-05-03
**Status:** Diskussionsgrundlage
**Verwandt:** [`positioning-2026-04-28.md`](./positioning-2026-04-28.md), [`honest-assessment-2026-04-28.md`](./honest-assessment-2026-04-28.md), [`competitor-piinuts-ch-2026-05-03.md`](./competitor-piinuts-ch-2026-05-03.md)

> **Kernthese:** Compliance-Pipeline = Flagschiff. Adapter = Distribution. Ghostwriter + Ticketsystem **nicht** als Standalone-SaaS, sondern als Open-Source-Reference-Apps (Trojanisches Pferd für Gaze-Adoption).

---

## Ausgangslage

User-Frage:
1. Soll Gaze + Compliance-Report-Pipeline + Laravel-Adapter (+ ggf. weitere Adapter) Flagschiff sein?
2. Sollen erste Consumer (Ghostwriter, Ticketsystem aus Dashboard-Projekt) als Standalone-Pakete geschnürt werden?

Befund Code-Stand (Dashboard-Repo `/Users/markusgottschau/IdeaProjects/Dashboard`):

- **Ghostwriter ist bereits Gaze-integriert in Production.** Gefunden:
  - `src/App/Features/GhostwriterGaze.php` — Feature-Flag
  - `src/Support/Ai/Sanitizer.php`, `GuardedAgentRunner.php`, `RestoringToolDecorator.php`
  - `src/Support/Ai/DTO/GazeInvocation.php`, `GuardedAgentResponse.php`
  - `src/Support/Ai/Exceptions/GazeDisabledException.php`
  - `src/Support/Ai/FeatureGate/GazeFeatureGate.php`
  - `src/Domain/Ghostwriter/Livewire/Admin/GazeLog.php` — Admin-Log-View
  - `config/gaze_boundary.php`
  - Hooks in `DraftGeneratorService`, `GhostwriterInboxProcessor`, `GhostwriterTranslationService`, `ProcessGhostwriterInboxJob`
- **Ticketsystem hat AI-Agents (`TicketAnalysisAgent`, `TicketCommentReplyAgent`) ohne Gaze-Schutz.** Klare Lücke.
- Dashboard-Stack: Laravel 12, Livewire 4, Filament/Nova, Octane, Pulse, Telescope, MySQL, Horizon, Spatie Media + Permission.

→ Echte Production-Integration vorhanden, nicht Hypothese. Wertvollstes Asset.

---

## Frage 1 — Flagschiff: Gaze + Compliance + Adapter?

**Ja, aber reframen. Adapter sind Distribution, nicht Produkt.**

### Klare Hierarchie

```
Compliance-Pipeline                 ← Hero-Story (Pricing, Pitch, Demo)
  └─ Gaze Runtime + gaze-audit      ← Engine (Trust-Anker)
      └─ Laravel-Adapter            ← Distribution
          └─ Python/Node/Go-Adapter ← zukünftige Distribution
```

- **Hero-Demo:** signierter PDF-Compliance-Report, deterministische Token-Herkunft, GDPR Art. 5(2)-Nachweis. Hat sonst keiner (siehe Wettbewerbs-Doc).
- **Engine:** Gaze v0.5 + `gaze-audit`-Crate. Reife steht.
- **Adapter:** Distribution-Channel. "Gaze for Laravel" → "Gaze for Python" → "Gaze for Node". Pflastersteine, keine Produkt-Linie.

### Was nicht funktioniert

- Adapter als gleichberechtigte Produkte verkaufen → Burnout-Trajektorie. Honest-Assessment-Warnung "4 Repos + Multi-Language ist viel" gilt.
- "Wir sind die Adapter-Firma" → spielt Tier-2-Gateways in die Karten (Kong, LiteLLM bauen Adapter "kostenlos" mit).
- Compliance-Story mit Adapter-Story vermischen → Käufer versteht nichts. Eine Botschaft pro Landing-Page.

### Pricing-Konsequenz

| Komponente | Pricing | Begründung |
|---|---|---|
| Gaze Engine (Core) | OSS Apache-2.0 | Trust-Anker, kein Verkaufs-Hebel |
| Laravel-/Python-/Node-Adapter | OSS, kostenlos | Distribution, kein Engpass |
| Premium-Recognizer-Pakete | Subscription (Hebel B) | Branchen-Wissen ist Wert |
| **Compliance-Cloud + signierte Reports** | **Subscription (Hebel A+C)** | **Kern-Verkaufs-Hebel** |
| Audit-Cloud (Bronze/Silver/Gold) | Usage-Based | Wert skaliert mit Volumen |
| Enterprise (On-Prem-Audit + SLA) | Contract | Höchste ACVs |

**Faustregel:** Wenn ein neuer Adapter "wir verkaufen jetzt PHP-Lizenzen" suggeriert → falscher Hebel. Adapter sind Lead-Magnet für Compliance.

---

## Frage 2 — Ghostwriter + Ticketsystem als Standalone-Pakete?

**Vorsicht. Drei Fallen.**

### Falle 1 — Scope-Explosion

Aktueller Stand: 4 Repos (`gaze`, `gaze-laravel`, `gaze-website`, externer `gaze-lens`) + Dashboard. Zwei zusätzliche Standalone-Produkte = **6+ Maintenance-Targets für 2 Personen**. Pivot-Trigger aus Honest-Assessment ("Disziplin bei Scope kritisch") feuert sofort.

### Falle 2 — Anderer ICP

| Produkt | Käufer | Sales-Trichter |
|---|---|---|
| Gaze | Backend-Dev / SRE / DPO | Compliance-Pain |
| Ghostwriter | Support-Team / Solo-Dev | Helpdesk-Effizienz |
| Ticketsystem | Kleines Team mit Helpdesk | Workflow-Tool |

Drei verschiedene Käufer, drei verschiedene Onboarding-Flows, drei verschiedene Support-Anfragen. **3-Firmen-Problem, nicht 1-Firmen-Multi-Produkt.**

### Falle 3 — Markt-Sättigung Helpdesk-AI

Ghostwriter-Kategorie: Help Scout AI, Intercom Fin, Zendesk AI, Freshdesk Freddy, Kayako, Tidio, hundertfach kleine Laravel-Pakete (Spatie etc.).

Ticketsystem: Linear, GitHub Issues, Plane, Jira-Klone, Jetzt+AI-Welle.

Beide brutal überlaufen. Differenzierung "läuft mit Gaze" ist für Helpdesk-Käufer Top-Caveat, nicht Top-Feature. Falscher Hebel.

---

## Was stattdessen funktioniert — Trojanisches Pferd

**Reference-Apps statt Standalone-SaaS.**

### Konzept

Ghostwriter + Ticketsystem als **Open-Source-Reference-Apps** für Gaze-Adoption. Apache-2.0, Composer-installierbar, offen.

```
composer require artistfy/ghostwriter
  → installiert Ghostwriter in Laravel-App
  → required dependency: naoray/gaze-laravel
  → first-run wizard: "Free Gaze key holen für Compliance-Reports?"
```

### Wirkung (5 Hebel)

1. **Distribution-Hebel.** Jeder Laravel-Dev der Helpdesk-AI baut sucht "laravel ai support email" → eure App ist OSS-First-Hit → installt Gaze automatisch.
2. **Trust-Beweis.** "Hier läuft Gaze in Production bei Artistfy" — nicht Marketing, sondern Code zum Anschauen.
3. **Recognizer-Discovery.** Künstler-Namen, Song-IDs, Order-IDs → Custom-Recognizer-Use-Case live demonstriert. Dogfood für `gaze-recognizers`.
4. **Compliance-Report-Showcase.** Echter Ghostwriter-Trafik produziert echten Audit-Trail → echter Demo-Report. Verkaufs-Hammer für DPO-Persona.
5. **Kein Standalone-SaaS-Maintenance.** Keine User-Accounts, keine Subscriptions, keine SLA-Pflicht. Nur Source-Code + Docs + Issue-Tracker.

### Nicht-Ziele

- ❌ Subscription-Modell für Reference-Apps. Nichts kostet Geld.
- ❌ "Hosted Ghostwriter" als Mini-SaaS. Wäre 4. Cloud-Service.
- ❌ Premium-Features in Reference-App. Verschmutzt Trust-Botschaft.

---

## Konkrete Roadmap

### Sofort (Mai–Juni 2026)

1. **Whitepaper "How Artistfy ships GDPR-compliant AI customer support with Gaze".** 4 Seiten, Architektur-Diagram, Token-Beispiel, Compliance-Report-Skizze, Lessons-Learned. PDF auf gaze-website. Aufwand: 1 Woche. Kosten: 0 EUR. Macht glaubwürdiger als Tonic-Marketing weil **echt**.
2. **Ticketsystem mit Gaze nachziehen.** Pattern aus Ghostwriter (`GuardedAgentRunner` umhüllt `TicketAnalysisAgent` + `TicketCommentReplyAgent`). Nicht extrahieren. Nur Schutz nachziehen.

### Q3 2026

3. **Ghostwriter abstrahieren.** Artistfy-Spezifika rauslösen (Customer-/Release-Modell, MySQL-Views, Spatie-Media-Pflicht). Domain-Adapter-Pattern wie Laravel-Auth-Provider.
4. **`artistfy/ghostwriter` als OSS-Composer-Paket releasen.** Apache-2.0. Hard-Dependency auf `naoray/gaze-laravel`. Erstes Reference-App.

### Q4 2026

5. **Ticketsystem extrahieren** wenn Gaze-Pattern stabil + Ghostwriter-Release validiert. Gleicher OSS-Pfad.

### Was wir absichtlich NICHT bauen

- ❌ Standalone-SaaS für Ghostwriter / Ticketsystem
- ❌ Premium-Lock-in (Pro-Tier "Ghostwriter Plus")
- ❌ Hosted-Variante davon
- ❌ Multi-Tenant-Adminpanel für Ghostwriter

---

## Empfehlung in 3 Punkten

1. **Flagschiff = Compliance-Pipeline.** Gaze Engine + signierter Report ist die Hero-Story. Adapter sind Distributions-Pflicht-Kacheln, nicht eigene Produkte. Pricing nur auf Compliance + Engine.

2. **Ghostwriter open-sourcen als Reference-App.** Apache-2.0, `artistfy/ghostwriter` Composer-Paket. Hard-Dependency auf `naoray/gaze-laravel`. Trojanisches Pferd für Gaze-Adoption. **Kein Subscription, kein Support-Vertrag, kein Pricing.** Marketing-Asset, nicht Geschäftsbereich.

3. **Ticketsystem zuerst Gaze-integrieren, dann gleicher Open-Source-Pfad.** Reihenfolge: (a) `GuardedAgentRunner` einziehen, (b) Pattern stabilisieren, (c) abstrahieren, (d) als Reference-App releasen. Frühestens Q3 2026.

---

## Risiken / Was schiefgehen kann

- **Open-Source-Maintenance-Last.** Auch ohne SaaS-Pflicht = Issues, PRs, Bug-Reports. Mitigation: Klar im README "best-effort, no SLA, contributions welcome".
- **Artistfy-Domain leakt in Reference-App.** Wenn Customer/Release-Modell nicht sauber abstrahiert → Adopter installiert nichts. Mitigation: Vorab-Abstraktion zwingend, kein "wir machen das später".
- **Ghostwriter-Markt nimmt Reference-App ernst** und fordert Features (Multi-Tenancy, SaaS-Hosting, SLA) → Pull in Standalone-SaaS-Falle. Mitigation: Hartes "Nein, das ist Gaze-Demo, nicht Helpdesk-Produkt" im README + auf Issues.
- **Recognizer-Pakete werden hauptsächlich für Ghostwriter-Use-Cases gebaut** statt für tatsächliche Käufer-Branchen (Health, Finance). Mitigation: Recognizer-Roadmap nach Käufer-Pipeline priorisieren, nicht nach eigener Convenience.
- **Brand-Verwirrung.** "Artistfy" + "PIInuts" + "Gaze" + "Ghostwriter" → Käufer versteht Verhältnis nicht. Mitigation: Klare Brand-Hierarchie definieren bevor Reference-App released.

---

## Offene Fragen

1. Wer übernimmt Ghostwriter-Abstraktion? Naoray? Ihr selbst? Aufwand-Schätzung?
2. Welche Recognizer-Klassen braucht Ghostwriter heute echt? (Künstler-Name, Song-ID, Email, Adresse, IBAN — Liste komplett?)
3. Wann wird Ticketsystem-Gaze-Integration eingeplant? Vor oder nach Ghostwriter-Release?
4. Whitepaper-Ownership: wer schreibt die 4 Seiten, wer reviewt?
5. Lizenz für Reference-Apps: Apache-2.0 (gleich wie Gaze-Core geplant) oder MIT?
6. Naming: `artistfy/ghostwriter` (vendor Artistfy) vs. `gaze/ghostwriter-demo` (vendor Gaze, klar als Demo markiert)?
7. Ist Whitepaper-Veröffentlichung mit Artistfy-Brand abgestimmt? (Co-Marketing-Wert für beide Seiten — Artistfy = compliance-bewusst, Gaze = Production-Reference.)

---

## Caveat

Snapshot 2026-05-03. Empfehlung gilt unter Bootstrap-Annahme (kein VC). Mit Funding könnte Standalone-SaaS-Pfad sinnvoll werden — dann aber als eigenständiges Team, nicht parallel von 2 Gaze-Foundern. Reference-App-Pfad ist die Bootstrap-konservative Variante mit höchstem Hebel pro Aufwand.
