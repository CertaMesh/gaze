# gaze-inspection

Provider-neutral, bounded inspection delivery for Gaze.

The crate owns sensitive, zeroizing payload wrappers and the matched producer/consumer runtime.
Payload reveal is an explicit trusted declassification boundary: bytes deliberately delivered to a
callback or sink cannot be revoked. Metadata-only delivery never includes content-derived values.

Metadata-only sinks can still observe event existence, closed stage/order/status, broad duration
buckets, queue outcomes, and the wall-clock cadence of their own callbacks. The runtime does not
claim traffic-analysis resistance and exposes no exact timestamp or fine-grained duration field.

`InstalledInspectionProducerV1::begin_logical()` remains nonblocking and may return
`Contended`. Control threads can use `begin_logical_blocking()` after a purge acknowledgement
to acquire the registration lock before checking readiness. It still rejects purging, disabled,
and closed registrations; keep it off the capture path.
