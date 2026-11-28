- The Nym safety net no longer reports a standalone run of more than eight
  digits, such as a nine-digit device serial, as a licence plate. The longest
  all-digit plate scheme we know of, Israel's, has eight digits. The refused
  span is reported as `nym_license_plate_digit_run_v1` telemetry with a
  metadata-only audit row.
