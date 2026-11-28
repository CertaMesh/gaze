- The Nym safety net no longer reports a standalone run of more than eight
  digits, such as a nine-digit device serial, as a licence plate. All-digit
  plates are at most eight digits long. The refused span is reported as
  `nym_license_plate_digit_run_v1` telemetry with a metadata-only audit row.
