use criterion::{black_box, criterion_group, criterion_main, Criterion, Throughput};
use std::fs;
use std::path::{Path, PathBuf};
use swift_schema::{infer_database_layout, FieldTypeSchema, SchemaCatalog};

fn repo_root() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .and_then(Path::parent)
        .expect("crate should live under the workspace crates directory")
        .to_path_buf()
}

fn load_catalog() -> SchemaCatalog {
    let schema_dir = repo_root().join("examples/schemas");
    let mut entries = fs::read_dir(schema_dir)
        .expect("schemas directory should be readable")
        .map(|entry| entry.expect("schema entry should be readable").path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("yaml"))
        .collect::<Vec<_>>();
    entries.sort();

    let mut field_types = Vec::new();
    let mut messages = Vec::new();
    for path in entries {
        let source = fs::read_to_string(path).expect("schema should be readable");
        let mut schema = SchemaCatalog::from_yaml_str(&source).expect("schema should parse");
        for field_type in schema.field_types.drain(..) {
            if let Some(existing) = field_types
                .iter()
                .find(|existing: &&FieldTypeSchema| existing.name == field_type.name)
            {
                assert_eq!(
                    existing, &field_type,
                    "duplicate field type definitions must match"
                );
                continue;
            }
            field_types.push(field_type);
        }
        messages.append(&mut schema.messages);
    }

    let catalog = SchemaCatalog {
        field_types,
        messages,
    };
    catalog
        .validate_rendering()
        .expect("example schema catalog should validate for rendering");
    catalog
}

fn infer_database_layout_benches(c: &mut Criterion) {
    let catalog = load_catalog();
    let layout = infer_database_layout(&catalog);
    assert!(!catalog.messages.is_empty());
    assert!(!layout.tables.is_empty());

    let mut group = c.benchmark_group("swift_schema_infer_database_layout");
    group.throughput(Throughput::Elements(catalog.messages.len() as u64));
    group.bench_function("full_example_schema_corpus", |bench| {
        bench.iter(|| {
            let layout = infer_database_layout(black_box(&catalog));
            black_box(layout);
        });
    });
    group.finish();
}

criterion_group!(benches, infer_database_layout_benches);
criterion_main!(benches);
