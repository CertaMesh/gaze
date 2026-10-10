# Governance

## Maintainers

Markus Gottschau (Ireland, founder and lead maintainer) and Krishan Koenig
(Germany, co-maintainer) have equal merge rights across the `CertaMesh` GitHub
organization. Either may block changes that violate the licence, open-detection
commitments, or the north star: zero PII leaks between agent and data owner.
Adding a maintainer requires both maintainers' agreement and an update here.

## Copyright

Contributors retain copyright. There is no Contributor Licence Agreement (CLA)
or copyright transfer. Every commit needs a Developer Certificate of Origin
(DCO) sign-off (`git commit -s`), confirming the right to contribute under the
project licence. No maintainer, sponsor, or acquirer can relicense unreplaced
contributions without their contributors' agreement.

## Licence

Gaze uses Apache-2.0 OR MIT, at the user's choice; both files are at the repo
root. Published versions remain available under those permissive OSI-approved
licences, including to commercial competitors. Changing future versions'
licence requires contributor agreement and a public explanation before landing.

## Contributor expectations

Submit GitHub PRs with DCO sign-off on every commit. Code must be compatible
with Apache-2.0 OR MIT; incompatible GPL, AGPL, or proprietary code is rejected.
Read [CONTRIBUTING](../../CONTRIBUTING.md) for gates and fixtures and
[Code of Conduct](../../CODE_OF_CONDUCT.md) for community rules.

Recognizers, validators, locale packs, and the detection pipeline stay public.
Every roadmap PII class ships in public `gaze-recognizers` rulepacks; no class
moves behind payment. Commercial features belong in separate repositories,
never OSS stubs, feature flags, or shims that steer adopters toward payment.

## How decisions get made

| Change | Decision |
|---|---|
| Bug fix or routine change | Any maintainer may review and merge |
| New recognizer class | Public issue with class, locale, validator strategy, and fixture plan; both approve, or one approves with no concern from the other within a reasonable window |
| Roadmap or milestone | Public issue, maintainer consensus, written rationale |
| Licence, governance, or security posture | Explicit agreement from both, documented and announced before taking effect |

If discussion cannot resolve disagreement, keep the conservative choice:
do not merge or weaken a licence, security, or audit guarantee.

## Commercial activities and the commons

The maintainers operate Empire2 Ltd (in formation in Ireland), which builds
commercial products around Gaze. Its value comes from operations: hosted audit
dashboards, compliance reports, vertical curation, and on-premise support.
A vertical detector must land in public `gaze-recognizers` first; commercial
software may enable it through configuration. Changes to these commitments
must be published here before they take effect.

## Reporting concerns

Use [Code of Conduct](../../CODE_OF_CONDUCT.md) for conduct reports,
[Security](../../SECURITY.md) for coordinated disclosure, or a GitHub issue
with the `governance` tag for governance concerns.
