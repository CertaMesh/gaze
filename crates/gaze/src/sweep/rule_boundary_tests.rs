// Each rule gets both private IDs and public syntax on the adjacent boundary.
fn check_boundary(pipeline: &crate::Pipeline, input: &str, values: &[&str]) {
    use crate::{CleanDocument, RawDocument, Scope, Session};
    let session = Session::new(Scope::Ephemeral).unwrap();
    let CleanDocument::Text(clean) = pipeline
        .redact(&session, RawDocument::Text(input.into()))
        .unwrap()
    else {
        panic!("text")
    };
    let entries = session.snapshot_entries();
    let mut expected = input.to_owned();
    for value in values {
        let entry = entries
            .iter()
            .find(|entry| entry.raw == *value)
            .unwrap_or_else(|| panic!("missing {value:?} in {input:?}: {entries:?}"));
        expected = expected.replace(value, &entry.token);
    }
    assert_eq!(clean, expected, "carrier changed: {input:?}; {entries:?}");
    assert_eq!(
        pipeline.restore_strict_text(&session, &clean).unwrap(),
        input
    );
}

#[test]
fn rule_boundary_references_and_types() {
    let pipeline = labelled_pipeline();
    let boundary_case = |input: &str, values: &[&str]| check_boundary(&pipeline, input, values);
    let mut count = 0;
    for label in ["customer_id", "employee_id", "person_id"] {
        for prefix in ["CID", "EID", "SID_", "UID", "INT", "U", "cid"] {
            for digits in ["731284", "88213", "7731"] {
                let value = format!("{prefix}{digits}");
                for carrier in [
                    "{L}={V}",
                    "level=info {L}={V} status=ok",
                    "GET /orders?{L}={V}&page=2",
                    "{L} = {V}",
                ] {
                    let input = carrier.replace("{L}", label).replace("{V}", &value);
                    boundary_case(&input, &[&value]);
                    count += 1;
                }
            }
        }
        for reference in [
            "customer_id2",
            "employee_id2",
            "newId2",
            "x1",
            "cust1",
            "e2",
            "cid2",
        ] {
            boundary_case(&format!("{label} = {reference}"), &[]);
            count += 1;
        }
        for dtype in [
            "int8", "int4", "INT8", "Int32", "Int64", "UInt64", "uint128", "i128", "u16",
            "float32", "float64", "f64", "serial8",
        ] {
            boundary_case(&format!("{label}: {dtype}\ncopy {dtype}"), &[]);
            count += 1;
        }
    }
    boundary_case("Employee ID: U16", &["U16"]);
    count += 1;
    boundary_case("Customer ID: C-731284(old) was merged", &["C-731284"]);
    count += 1;
    for value in ["uuid4", "sha256", "b64encode"] {
        boundary_case(&format!("customer_id = {value}(raw)"), &[]);
        count += 1;
    }
    assert_eq!(count, 317);
}

#[test]
fn rule_boundary_sql_context_and_keywords() {
    let pipeline = labelled_pipeline();
    let boundary_case = |input: &str, values: &[&str]| check_boundary(&pipeline, input, values);
    let mut count = 0;
    for keyword in [
        "AND",
        "OR",
        "NOT",
        "WHERE",
        "ORDER",
        "GROUP",
        "BY",
        "LIMIT",
        "OFFSET",
        "HAVING",
        "JOIN",
        "ON",
        "SET",
        "VALUES",
        "RETURNING",
        "UNION",
        "NOT NULL",
        "PRIMARY KEY",
        "REFERENCES",
    ] {
        for spelling in [keyword.to_owned(), keyword.to_lowercase()] {
            for prefix in [
                "SELECT * FROM t WHERE ",
                "UPDATE t SET ",
                "SELECT *\nFROM t\nWHERE ",
            ] {
                let input = format!("{prefix}customer_id = 731284 {spelling} other");
                boundary_case(&input, &["731284"]);
                count += 1;
            }
        }
    }
    for parameter in ["@p0", "@p1", "@name2", ":name", "$1"] {
        for prefix in [
            "SELECT * FROM t WHERE ",
            "UPDATE t SET ",
            "SELECT *\nFROM t\nWHERE ",
        ] {
            boundary_case(&format!("{prefix}customer_id = {parameter}"), &[]);
            count += 1;
        }
    }
    for prose in [
        "Please select the right account: ",
        "Select one. ",
        "We will create table backups later\n",
        "Don't alter table rows by hand.\n",
        "CREATE TABLE t (x int);\n",
        "CREATE TABLE t (x int)\n",
    ] {
        for (start, end) in [("(", ")"), ("[", "]")] {
            let input = format!("{prose}Customer ID {start}C-731284{end}");
            boundary_case(&input, &["C-731284"]);
            count += 1;
        }
    }
    for schema in [
        "CREATE TABLE t (\ncustomer_id (C-731284)\n)",
        "CREATE TABLE t (\ncustomer_id (731284)\n)",
    ] {
        boundary_case(schema, &[]);
        count += 1;
    }
    boundary_case("See CREATE TABLE t (\ncustomer_id (C-731284)\n)", &[]);
    count += 1;
    assert_eq!(count, 144);
}

#[test]
fn rule_boundary_markup_grammar() {
    let pipeline = labelled_pipeline();
    let boundary_case = |input: &str, values: &[&str]| check_boundary(&pipeline, input, values);
    let mut count = 0;
    for tag in ["a", "td", "button", "svg", "math", "custom-field"] {
        for attribute in [
            "href=https://example.invalid/c",
            "xml:lang=en",
            ":key=k",
            "href=/x?a=1",
            "style=color:red;",
            "@click=go",
            "v-bind.key=k",
            "ng-model=x",
            "x-bind=x",
        ] {
            for close in ["", "/", ">v</{T}"] {
                let input = format!("<{tag} {attribute} data-customer-id=C-731284{close}>")
                    .replace("{T}", tag);
                boundary_case(&input, &[]);
                count += 1;
            }
        }
        for close in ["/>", ">v</{T}>"] {
            let input = format!("<{tag} customAttr=C-731284 customer_id=C-731284{close}")
                .replace("{T}", tag);
            // A matching close or self-close establishes real markup even with unknown attrs.
            boundary_case(&input, &[]);
            count += 1;
        }
    }
    for input in [
        "while i <n and customer_id=731284 > 0:",
        "run <input customer_id=731284 >out",
        "WHERE a <b AND customer_id=731284 OR c >d",
        "keep it <short\ncustomer_id=731284\nthanks >",
        "customer_id: 731284\nif x <y z=731284 >",
    ] {
        boundary_case(input, &["731284"]);
        count += 1;
    }
    for attr in ["data-x=1;", "data-x=a,b", "data-x=a:b", "data-x=a=b"] {
        boundary_case(
            &format!("<td {attr}\ndata-customer-id=C-731284>v</td>"),
            &[],
        );
        count += 1;
    }
    for tag in ["Row", "CustomerPanel"] {
        for close in ["/>", ">value</{T}>"] {
            boundary_case(
                &format!("<{tag} customer_id=C-731284{close}").replace("{T}", tag),
                &[],
            );
            count += 1;
        }
        boundary_case(&format!("run <{tag} customer_id=731284 >out"), &["731284"]);
        count += 1;
    }
    for suffix in ["", "\n", " ", "<br/>"] {
        boundary_case(&format!("<input customer_id=C-731284>{suffix}"), &[]);
        count += 1;
    }
    boundary_case("<input\n customer_id=C-731284>", &[]);
    count += 1;
    boundary_case("run <input customer_id=731284 >out", &["731284"]);
    count += 1;
    boundary_case("run <input customer_id=731284>out", &["731284"]);
    count += 1;
    for attribute in ["Customer ID", "Employee ID", "customerId", "MEMBER_ID"] {
        boundary_case(&format!("<input {attribute}=C-731284>"), &[]);
        count += 1;
    }
    for keyword in ["AND", "OR", "NOT", "WHERE", "SELECT", "FROM", "SET"] {
        boundary_case(
            &format!("while x <b {keyword} customer_id=731284 >out"),
            &["731284"],
        );
        count += 1;
    }
    assert_eq!(count, 207);
}

#[test]
fn rule_boundary_starts_wrappers_and_grouping() {
    let pipeline = labelled_pipeline();
    let boundary_case = |input: &str, values: &[&str]| check_boundary(&pipeline, input, values);
    let mut count = 0;
    for label in ["Customer ID", "Employee ID", "Member ID"] {
        for (start, value, end) in [
            (": ", "#731284", ""),
            (": ", "#FABLE731284", ""),
            (" - ", "C-731284", ""),
            (" – ", "731284", ""),
            (" — ", "C-731284", ""),
            (": ", "AB=12345", ""),
            (": ", "+YQ==", ""),
            (": '", "opaque+YQ==", "'"),
            (": ", "AB 12 34 56", ""),
            (" (", "731284", ")"),
            (": [", "731284", "]"),
            (": §", "C-731284", "§"),
            (": ⟨", "C-731284", "⟩"),
        ] {
            boundary_case(&format!("{label}{start}{value}{end}"), &[value]);
            count += 1;
        }
        for public in [
            "#d3d3d3",
            "#a1b2c3d4",
            "#G123",
            "#1a2b3c",
            "#fff",
            "a==b",
            "+y",
            "=> 1",
            "req.params.id",
            "${VALUE}",
            "{{VALUE}}",
        ] {
            boundary_case(&format!("{label}: {public}"), &[]);
            count += 1;
        }
    }
    boundary_case(":root { --customer-id: #1a2b3c; }", &[]);
    count += 1;
    boundary_case("record_id(42)", &[]);
    count += 1;
    assert_eq!(count, 74);
}
