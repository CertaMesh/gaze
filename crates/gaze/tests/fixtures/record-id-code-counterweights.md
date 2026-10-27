# Code and SQL counterweights

`record-id-code-counterweights.json` contains synthetic inputs for the record-ID
structure oracle. The pandas example retains the public `order_id` column to
prove that a rejected dtype cannot seed replacements in another column.

This is test data, loaded only by the `#[cfg(test)]` sweep tests. It registers no
PII class or runtime tenant rule. [The fixture rules](../../../../CONTRIBUTING.md#tenant-class-names-in-tests)
explicitly exclude tests from the production-source gate. No gate exemption or
rule change is used.
