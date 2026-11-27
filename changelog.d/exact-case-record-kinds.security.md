- A caller-known record value is no longer left raw because it appears in its
  declared spelling. Match kinds are now ordered: an exact occurrence satisfies
  every looser enabled kind, so a single name declared as `Qorvex` is protected
  as `Qorvex` wherever `qorvex` already was, and a same-case full name with
  extra spaces is protected wherever its upper-case copy was. Common single
  names still need corroboration in every letter case, and that set now also
  includes English given names that are ordinary words (`Summer`, `Dawn`),
  generated from SSA given names and SCOWL en_US words. Record dictionaries and
  the MCP host-context guess check share the rule.
