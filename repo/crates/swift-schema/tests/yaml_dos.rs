use std::time::{Duration, Instant};
use swift_schema::SchemaCatalog;

#[test]
fn alias_expansion_payload_rejects_or_completes_quickly() {
    let yaml = r#"
l0: &l0 ["x", "x", "x", "x", "x", "x", "x", "x"]
l1: &l1 [*l0, *l0, *l0, *l0, *l0, *l0, *l0, *l0]
l2: &l2 [*l1, *l1, *l1, *l1, *l1, *l1, *l1, *l1]
l3: &l3 [*l2, *l2, *l2, *l2, *l2, *l2, *l2, *l2]
l4: &l4 [*l3, *l3, *l3, *l3, *l3, *l3, *l3, *l3]
field_types: *l4
messages: []
"#;

    let started = Instant::now();
    let result = SchemaCatalog::from_yaml_str(yaml);

    assert!(
        started.elapsed() <= Duration::from_millis(500),
        "alias expansion YAML parse exceeded 500 ms"
    );
    assert!(
        result.is_err(),
        "alias expansion YAML should not load as a valid schema"
    );
}
