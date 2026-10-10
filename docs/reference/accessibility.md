# Accessibility

## CLI (`gaze clean` / `gaze restore`)

The CLI uses plain text and stdin/stdout pipes. Diagnostics explain errors in
text; color never carries meaning alone.

ANSI styling uses one `IsTerminal` gate:

| Condition | Styling |
|---|---|
| Non-empty `NO_COLOR` | Off |
| Non-empty `CLICOLOR_FORCE`, with no non-empty `NO_COLOR` | On |
| Otherwise | Terminal streams only |

`gaze mcp doctor` always prints `pass`, `warn` and `fail`, even when the state
column is colored. JSON is never colored. Tests cover forced color,
`NO_COLOR`, non-TTY output, state words and JSON.

## Documentation

Use one H1, ordered heading levels, image alt text, language-tagged code blocks
and simple tables with header rows.

## Dashboard UI

The opt-in `gaze proxy serve --dashboard` UI targets WCAG 2.2 AA. See the
[verification protocol and 44-state evidence](dashboard/accessibility-and-visual-verification.md)
and [browser security rules](dashboard/browser-security.md).

## Future UI surfaces

The `gaze-website` marketing site and future audit viewers target at least
WCAG 2.1 AA: keyboard navigation, contrast, visible focus and accessible
control labels. Accessibility regressions must not ship.
