# Release Process

## Homebrew Tap Location

Decision for v0.4.6 S6 (#184): keep Homebrew repo-local until the organization creates an explicit public tap and release publication target.

Current state:

- The formula source lives in this repository at `dist/homebrew/gaze.rb`.
- No public `EmpireTwo/tap` or `EmpireTwo/homebrew-tap` repository is visible from this repo-local audit.
- `EmpireTwo/gaze` is private, so public Homebrew installation is not a supported path yet.
- `.github/workflows/release.yml` intentionally remains artifact-only: it builds and uploads GitHub release assets, but does not push formula updates to an external tap.
- Modern Homebrew rejects direct install/info commands for formula files outside a tap, so local smoke means staging the formula into a scratch tap rather than installing `./dist/homebrew/gaze.rb` directly.

Axis-5 rationale: documenting the repo-local formula is more ergonomic than advertising a tap that adopters cannot use. It gives collaborators a concrete smoke path while keeping public install instructions honest and reversible when a public tap is created.

Local smoke for maintainers:

```bash
brew tap-new EmpireTwo/gaze-smoke
cp dist/homebrew/gaze.rb "$(brew --repo EmpireTwo/gaze-smoke)/Formula/gaze.rb"
brew info EmpireTwo/gaze-smoke/gaze
brew untap EmpireTwo/gaze-smoke
```

Future public tap work is org-level operations outside this repo PR. When a public tap exists, update this document, the README install section, and `.github/workflows/release.yml` together so the formula location, adopter instructions, and release automation agree.
