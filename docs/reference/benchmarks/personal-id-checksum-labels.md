# Labelled personal national IDs with checksum errors

Generator v18 treats explicitly labelled Aadhaar and NIR values as personal data
when their checksum fails. A mistyped personal national ID still identifies a
person; checksum validity is diagnostic metadata, not permission to treat that
value as benign. This extends the personal-ID ownership ruling already applied
to labelled Steuer-ID, CPF, BSN and NHS twins and to IBAN/payment-card gold.

Only the following cells change, identically in each partition:

| Class | Prose cells | Log cells | JSON cells | New gold bytes |
| --- | ---: | ---: | ---: | ---: |
| Aadhaar | 10 | 10 | 10 | 360 |
| NIR | 10 | 10 | 10 | 450 |
| Total | 20 | 20 | 20 | 810 |

These 60 documents move from benign layer D to gold layer A with `invalid`
validity. Their document IDs change only the layer component. Text, inserted
values, UTF-8 offsets, order, group/template identities, seeds and every other
cell stay fixed. Each partition has 5,018 A, 3,314 D and 1,390 R documents.
The [cell-level diff](personal-id-checksum-v18-cell-diff.json) lists every changed
record by partition and class. Tests compare every v17/v18 record, assert the exact family/surface counts
and preserve v17's corpus and contract byte pins. V18 regenerates twice with
identical hashes. Deliberately reverting one Aadhaar prose cell to benign fails
the pinned corpus and cell-diff tests; restoring the source makes them pass.

The existing personal national-ID checksum twins outside this coverage slice
already carry gold. CNPJ company identifiers and ETH-address checksum decoys
remain benign. Public `caseId`, account-nested order/ticket references and
`customer` / `batch_reference` coverage controls also remain benign. Inventory
and batch reference twins keep their original labels even when their numeric
shape resembles a personal national ID. No recognizer or gate-credit exception
changes. All values remain synthetic.

The stricter gold adds measured leaked bytes for every displayed release.
Any reduced D false positives arise from moving personal data out of the benign
population, and do not establish a runtime precision improvement. The current
[release table](README.md#measured-adjacency-layer-history) is generated from
fresh tag-bound v18 measurements. The frozen v17 corpus, contract, scorecards,
observations and ledger remain available for comparison. Layer C inputs and
published scores are unchanged.
