# Governance

This document explains who decides what in the Gaze project, how contributions are accepted, and how Gaze remains a public commons even when its maintainers run commercial activities elsewhere.

It is deliberately short: governance for a small project should describe what actually happens, not aspire to a structure the project does not yet need.

## Maintainers

Gaze is currently maintained by two people:

- **Markus Gottschau** (Ireland) — founder, lead maintainer. Merge rights on all repositories under the `CertaMesh` GitHub organisation.
- **Krishan Koenig** (Germany) — co-maintainer. Merge rights on all repositories under the `CertaMesh` GitHub organisation.

Both maintainers have equal merge authority. Either can land changes; either can block changes that violate the licence, the recognizer-class commitments below, or the project's stated north star ("zero PII leaks between agent and data owner").

Adding a new maintainer requires explicit agreement from both current maintainers and is recorded by updating this file and the relevant `CODEOWNERS` entries.

## Copyright

Contributors retain copyright in their own contributions. The project does not require contributors to assign or transfer copyright.

There is **no Contributor Licence Agreement (CLA)**. The only contribution gate is the Developer Certificate of Origin (DCO) — every commit must be signed off with `git commit -s`, asserting that the contributor has the right to submit the work under the project's licence. This is the same mechanism used by the Linux kernel and Docker.

Because copyright stays distributed across all contributors and there is no CLA giving any single entity unilateral re-licensing authority, **the project cannot be silently re-licensed** by any later maintainer, sponsor, or acquirer without the agreement of every contributor whose code has not yet been replaced.

## Licence

The project is published under **Apache-2.0 OR MIT** at the user's option — the standard Rust ecosystem licence posture. Both licence files live at the repository root (`LICENSE-APACHE`, `LICENSE-MIT`).

Both licences are OSI-approved, permissive, and irreversible for any version already published. Once a release is tagged and pushed, that version is permanently available under these terms to everyone — including any commercial competitor.

The maintainers reserve the right to change the licence on **future** versions, subject to contributor agreement under DCO. Any such change would be announced publicly with reasoning before it landed.

## Contributor expectations

Contributions are accepted via GitHub pull request. The expectations are:

1. **DCO sign-off** on every commit (`git commit -s`).
2. **Code is licensed under Apache-2.0 OR MIT.** Contributors confirm this by signing off; PRs that include code with an incompatible licence (GPL, AGPL, proprietary) are not merged.
3. **No proprietary code paths in OSS repositories.** Recognizers, validators, locale packs, and the detection pipeline live in public repositories under the project's licence. Premium operational features (hosted dashboards, support SLAs, vertical compliance bundles) live in separate repositories and never sneak into the OSS codebase as feature flags or stub implementations.
4. **No taxonomy class migrates to a paid product.** Every PII class on the recognizer roadmap is committed to ship in the public `gaze-recognizers` rulepacks. The detection floor is open-source; commercial value is built on coordination, distribution, and operations, not on withholding detection.
5. **No bait-and-switch.** Because there is no CLA, the maintainers do not have the unilateral power to take currently-published code closed-source. The DCO-only contribution model is itself the structural guarantee.

Contributors should also read `CONTRIBUTING.md` for the technical contribution workflow (gates, fixture conventions, test rituals) and `CODE_OF_CONDUCT.md` for community expectations.

## How decisions get made

The decision-making process is intentionally lightweight:

- **Bug fixes and routine changes:** any maintainer can review and merge.
- **New recognizer classes:** opened as a GitHub issue describing the class, target locale, validator strategy, and fixture plan. Discussion in the issue thread. Merge once both maintainers have signed off — or one maintainer has signed off and the other has not raised concerns within a reasonable window.
- **Roadmap and milestone decisions:** discussed in public issues or in `ROADMAP.md`. Maintainer consensus, with the rationale recorded in writing.
- **Licence, governance, or security-posture changes:** require explicit agreement from both maintainers, documented in this file (or its successor) and announced before the change takes effect.

Disagreements between maintainers are resolved by discussion. If a disagreement cannot be resolved, the more conservative option is taken — meaning: do not merge, do not change the licence, do not weaken a security or audit guarantee.

## Commercial activities and the commons

The maintainers operate a company (Empire2 Ltd, in formation in Ireland) that builds commercial products around Gaze. This raises a fair question: does the project remain a public commons in practice, or is the open-source repository slowly being hollowed out into a free trial of a paid product?

Three structural commitments answer this:

1. **DCO without CLA.** As above — there is no unilateral re-licensing authority. The currently-published code is permanently available under Apache-2.0 OR MIT.

2. **Open detection layer, forever.** Every PII recognizer, every validator, every locale pack, and the entire detection pipeline ships in the public repository under the project's licence. The commercial tier is built on **operational** value (hosted multi-tenant audit dashboards, compliance reports, vertical curation, on-premise support engagements) — never on holding back detection capability. If a vertical needs a new detector, it lands in `gaze-recognizers` first; the commercial layer can only enable it via configuration.

3. **Separate repositories.** Commercial features live in separate repositories from the OSS core. They never enter the OSS codebase as stubs, feature flags, or interface-only shims that quietly steer users toward the paid product.

If any of these commitments are ever weakened, this file gets updated openly first — not after the fact.

## Reporting concerns

- **Code of Conduct violations:** see `CODE_OF_CONDUCT.md` (Contributor Covenant) for reporting channels.
- **Security issues:** see `SECURITY.md` for coordinated-disclosure contact details.
- **Governance concerns:** open a GitHub issue tagged `governance`, or contact the maintainers directly via the emails listed in `CONTRIBUTING.md` if the matter is not appropriate for a public issue.
