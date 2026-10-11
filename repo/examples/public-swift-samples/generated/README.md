Generated structural samples
============================

One sample per schema produced by `scripts/gen-schemas-from-prowide.py`: mandatory fields
only, repeating sequences twice, dummy values (`X`, `REF`). They prove that each generated
schema's anchoring, required-field and cardinality rules are internally consistent. They are
not realistic messages and say nothing about field formats. Checked by
`crates/swift-schema/tests/upstream_fixtures.rs`.
