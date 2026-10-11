# BSN benign-twin label correction (generator v16)

A customer number identifies personal data. Generator v15 contradicted itself:
its CRM/HR cells scored labelled customer numbers as gold, but its layer D BSN
benign twins put nine-digit references under `customer_no` (dev) and
`customerNumber` (test), with no gold. A recognizer protecting that personal
identifier consequently incurred false-positive bytes.

Generator v16 corrects only those labels, in ten log and ten JSON documents per
partition. Labels cycle through an authored semantic vocabulary: batch, shipment
and equipment references (`batch_number`, `sensor_serial`, `shipment_reference`
in dev; `lotNumber`, `deviceSerial`, `shipmentReference` in test). They identify
inventory or equipment without a person or account holder. The vocabulary comes
from those meanings; it neither imports nor mirrors recognizer expressions.
All digits, document IDs, ordering, prose twins, positive gold, repeat cells and
other metadata remain identical. The correction consumes no value RNG draws.

This correction can remove false-positive bytes for any recognizer that treats
customer numbers as personal data. Such a decrease is a corpus correction, not
a runtime precision improvement. No record-ID recognizer changes here. The
historical reconstruction reverses the label substitution for v3–v15, whose
corpus byte hashes remain pinned. Generator v14 scorecards, observations, ledger
and frozen contract are retired; v15 remains the previous retained version.
Older v4–v13 history and contracts remain temporarily because external consumers
still bind them; migrating those consumers is separate contract-pruning work.

## Measured effect

Fresh v16 measurements of v0.15.0, v0.15.1 and v0.16.0 retain identical leaked
bytes but add 18 false-positive bytes in D versus v15. The bound traces show
Nym protecting two nine-digit `deviceSerial` values as `custom:license_plate`.
The authored labels are unchanged after observing that result. Both v0.14.0
arms have identical byte counts to v15. All arms have zero refusals; historical
restore and manifest errors remain unchanged and visible in the release tables.
[Contract replays](bsn-twin-v16-contract-replay.json) bind the v1/v2/v4 results to
the committed observation records.

## Benign-cell audit

Both partitions were searched for customer, employee, patient, member, account,
user and client wording, including their ID/number forms, and German customer,
employee and account labels. Every generated layer D document and every layer
A/R document with recorded decoys was inspected. Source templates were searched
as well, including the known-record cells. All 1,024 committed primary layer C negatives were searched too. Their 256
EN/DE person/account-word hits belong to the two categories listed below.
Known-record counterweight source templates have no person-linked ID/number
labels. The layer C corpus and the separate dictionary-sequence and competitor
sets are unchanged by this generator.

| Cells | Scoring | Disposition |
| --- | --- | --- |
| C `commerce_identifiers`, 64 EN + 64 DE | No gold; every protected byte is FP | Retain: order/invoice/stock/batch keys explicitly have no customer or account linkage |
| C `invalid_identifiers`, 64 EN + 64 DE | No gold; every protected byte is FP | Retain: account-shaped references explicitly say invalid and unassigned; no personal owner is supplied |
| `ref_number_9`, log and JSON, ten each per partition | No gold; every protected byte is FP | Correct person-linked customer label in v16 |
| Record-ID English/German placeholders, four words per class/partition, plus one customer bridge; quoted-label variants likewise | No gold; inserted word is a placeholder decoy | Retain: schema wording supplies no identity value |
| Record-ID JSON `null`, `true`, `false`, ten per class/keyword/partition | No gold; keyword is a typed JSON decoy | Retain: these keywords supply no identifier |
| Public-resource record-ID prose, ten per class/shape/partition | No gold; inserted public-resource ID is a decoy | Retain: value belongs to `order_id`/`orderId`; customer ID explicitly belongs in a separate field |
| R `id_repeats`, dev IBAN/phone/tax-ID, ten per family | Identity repeats are gold; batch digits are decoys | Retain: the batch is explicitly unrelated to the customer |
| `ref_number_10`, test log/JSON, ten each | No gold; every protected byte is FP | Ambiguous: `caseId` does not specify a person or a case domain; retain pending ruling |
| `order_json_entries_array`, four per partition | No gold; every protected byte is FP | Ambiguous: public order/ticket codes nested inside `account`; retain pending ruling |
| `order_json_type_above`, four test documents | No gold; every protected byte is FP | Ambiguous: a ticket reference described as belonging to an account; retain pending ruling |
| `coverage_json`, seven test identifier families, ten each | No gold; `batch_reference` values are decoys | Ambiguous: non-person label nested under `customer`; retain pending ruling |
| `coverage_json_invalid`, test Aadhaar/CNPJ/ETH/NIR, ten each | No gold; checksum-invalid labelled identifiers are decoys | Ambiguous: explicit identifier labels under `customer`; retain pending ruling |

The ambiguous cells need a separate scope/scoring decision. This audit does not
claim that they are safe non-person references. Placeholder and keyword
counterweights intentionally retain person-linked labels to price false positives
on absent identifiers.
