#[cfg(feature = "phone-parser")]
use gaze_recognizers::Region;
use gaze_recognizers::ValidatorKind;
use gaze_types::{ValidatorFailReason, ValidatorOutcome};

fn assert_pass(kind: ValidatorKind, input: &str) {
    assert!(
        matches!(
            kind.validate(input),
            ValidatorOutcome::Pass {
                canonical_form: Some(_)
            }
        ),
        "{kind:?} should pass {input}"
    );
}

fn assert_fail(kind: ValidatorKind, input: &str, reason: ValidatorFailReason) {
    assert_eq!(kind.validate(input), ValidatorOutcome::Fail { reason });
}

#[test]
fn email_rfc_validator_reports_pass_and_fail() {
    assert_pass(ValidatorKind::EmailRfc, "alice@example.invalid");
    assert_fail(
        ValidatorKind::EmailRfc,
        "alice@example",
        ValidatorFailReason::EmailRfcRejected,
    );
}

#[cfg(feature = "phone-parser")]
#[test]
fn e164_phone_validator_reports_pass_and_fail() {
    assert_pass(ValidatorKind::E164Phone, "+4915550112233");
    assert_fail(
        ValidatorKind::E164Phone,
        "+99999999",
        ValidatorFailReason::PhoneE164Rejected,
    );
}

#[cfg(feature = "phone-parser")]
#[test]
fn national_phone_validator_reports_pass_and_fail() {
    assert_pass(
        ValidatorKind::E164PhoneNational(Region::De),
        "+49 1555 0112233",
    );
    assert_fail(
        ValidatorKind::E164PhoneNational(Region::Us),
        "+49 1555 0112233",
        ValidatorFailReason::PhoneNationalRegionMismatch,
    );
}

#[test]
fn luhn_validator_reports_pass_and_fail() {
    assert_pass(ValidatorKind::Luhn, "4111-1111-1111-1111");
    assert_fail(
        ValidatorKind::Luhn,
        "4111-1111-1111-1112",
        ValidatorFailReason::LuhnFailed,
    );
}

#[test]
fn iban_mod97_validator_reports_pass_and_fail() {
    assert_pass(ValidatorKind::IbanMod97, "DE89 3704 0044 0532 0130 00");
    assert_fail(
        ValidatorKind::IbanMod97,
        "DE99 3704 0044 0532 0130 00",
        ValidatorFailReason::IbanMod97Failed,
    );
}

#[test]
fn ipv4_parse_validator_reports_pass_and_fail() {
    assert_pass(ValidatorKind::Ipv4Parse, "192.0.2.1");
    assert_fail(
        ValidatorKind::Ipv4Parse,
        "192.0.2.300",
        ValidatorFailReason::Ipv4ParseFailed,
    );
}

#[test]
fn ipv6_parse_validator_reports_pass_and_fail() {
    assert_pass(ValidatorKind::Ipv6Parse, "2001:db8::1");
    assert_fail(
        ValidatorKind::Ipv6Parse,
        "2001::1::2",
        ValidatorFailReason::Ipv6ParseFailed,
    );
}

#[test]
fn bundled_ip_validators_name_documentation_vetoes_without_changing_parsing() {
    let ipv4 = ValidatorKind::parse("ipv4_parse_non_documentation").unwrap();
    for address in ["192.0.2.0", "192.0.2.255", "198.51.100.7", "203.0.113.1"] {
        assert_fail(ipv4, address, ValidatorFailReason::Ipv4DocumentationRange);
        assert_pass(ValidatorKind::Ipv4Parse, address);
    }
    for address in ["192.0.3.1", "192.168.1.1", "198.51.101.7"] {
        assert_pass(ipv4, address);
    }
    assert_fail(ipv4, "192.000.2.1", ValidatorFailReason::Ipv4ParseFailed);

    let ipv6 = ValidatorKind::parse("ipv6_parse_non_documentation").unwrap();
    for address in [
        "2001:db8::1",
        "2001:0DB8:ffff::1",
        "2001:db8:0:0:0:0:0:1",
        "::ffff:192.0.2.1",
        "::192.0.2.1",
    ] {
        assert_fail(ipv6, address, ValidatorFailReason::Ipv6DocumentationRange);
        assert_pass(ValidatorKind::Ipv6Parse, address);
    }
    for address in ["2001:db9::1", "fe80::1", "::1", "::ffff:192.0.3.1"] {
        assert_pass(ipv6, address);
    }
    assert_fail(ipv6, "2001::1::2", ValidatorFailReason::Ipv6ParseFailed);
}

#[test]
fn eth_eip55_validator_reports_pass_and_fail() {
    assert_pass(
        ValidatorKind::EthEip55,
        "0x52908400098527886E0F7030069857D2E4169EE7",
    );
    assert_fail(
        ValidatorKind::EthEip55,
        "0x52908400098527886e0F7030069857D2E4169EE7",
        ValidatorFailReason::EthEip55ChecksumFailed,
    );
}
