use criterion::{black_box, criterion_group, criterion_main, Criterion, Throughput};
use swift_core::parse_message;
use swift_db::{materialize_message, InboundMessage};
use swift_schema::SchemaCatalog;

fn mt540_catalog() -> SchemaCatalog {
    let catalog =
        SchemaCatalog::from_yaml_str(include_str!("../../../examples/schemas/mt540.yaml"))
            .expect("MT540 schema should parse");
    catalog.validate().expect("MT540 schema should validate");
    catalog
        .validate_rendering()
        .expect("MT540 schema should validate for rendering");
    catalog
}

fn mt540_inbound() -> InboundMessage {
    InboundMessage {
        id: "bench-mt540".to_string(),
        message_type: "MT540".to_string(),
        body: include_str!("../../../examples/mt540_sample.fin").to_string(),
    }
}

fn materialize_message_benches(c: &mut Criterion) {
    let catalog = mt540_catalog();
    let inbound = mt540_inbound();
    let parsed = parse_message(inbound.body.as_bytes());
    assert!(parsed.diagnostics.is_empty());

    let output = materialize_message(&catalog, &inbound, &parsed).expect("MT540 materializes");
    assert!(!output.fields.is_empty());
    assert!(!output.normalized_rows.is_empty());

    let mut group = c.benchmark_group("swift_db_materialize_message");
    group.throughput(Throughput::Elements(parsed.fields.len() as u64));
    group.bench_function("mt540_sample", |bench| {
        bench.iter(|| {
            let output =
                materialize_message(black_box(&catalog), black_box(&inbound), black_box(&parsed))
                    .expect("MT540 materializes");
            black_box(output);
        });
    });
    group.finish();
}

criterion_group!(benches, materialize_message_benches);
criterion_main!(benches);
