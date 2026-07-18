use criterion::{black_box, criterion_group, criterion_main, BatchSize, Criterion, Throughput};
use duckdb::Connection;
use std::collections::BTreeMap;
use std::time::Duration;
use swift_db::{NormalizedRow, ParsedOutputBatch, ParsedSink};
use swift_duckdb::{DuckDbInboundConfig, DuckDbStore};
use swift_schema::{ColumnLayout, DatabaseLayout, LogicalColumnType, TableLayout};

const ROW_COUNT: usize = 10_000;

fn layout() -> DatabaseLayout {
    DatabaseLayout {
        tables: vec![TableLayout {
            name: "settlement_instruction".to_string(),
            columns: vec![
                ColumnLayout {
                    name: "message_id".to_string(),
                    logical_type: LogicalColumnType::Text,
                    required: true,
                },
                ColumnLayout {
                    name: "sequence_path".to_string(),
                    logical_type: LogicalColumnType::Text,
                    required: true,
                },
                ColumnLayout {
                    name: "sender_reference".to_string(),
                    logical_type: LogicalColumnType::Text,
                    required: false,
                },
                ColumnLayout {
                    name: "settlement_amount".to_string(),
                    logical_type: LogicalColumnType::Decimal,
                    required: false,
                },
            ],
        }],
    }
}

fn batch() -> ParsedOutputBatch {
    let normalized_rows = (0..ROW_COUNT)
        .map(|index| NormalizedRow {
            table: "settlement_instruction".to_string(),
            values: BTreeMap::from([
                ("message_id".to_string(), format!("msg-{index:05}")),
                ("sequence_path".to_string(), "GENL[0]".to_string()),
                ("sender_reference".to_string(), format!("SEME{index:05}")),
                ("settlement_amount".to_string(), format!("{index}.00")),
            ]),
        })
        .collect();

    ParsedOutputBatch {
        raw_messages: Vec::new(),
        fields: Vec::new(),
        parse_errors: Vec::new(),
        normalized_rows,
    }
}

fn store() -> DuckDbStore {
    let conn = Connection::open_in_memory().expect("in-memory DuckDB opens");
    let mut store = DuckDbStore::new(conn, DuckDbInboundConfig::default());
    swift_db::MigrationSink::apply_layout(&mut store, &layout()).expect("layout applies");
    store
}

fn write_batch_benches(c: &mut Criterion) {
    let batch = batch();
    assert_eq!(batch.normalized_rows.len(), ROW_COUNT);

    let mut group = c.benchmark_group("swift_duckdb_write_batch");
    group.sample_size(10);
    group.measurement_time(Duration::from_secs(20));
    group.throughput(Throughput::Elements(ROW_COUNT as u64));
    group.bench_function("normalized_10k_rows", |bench| {
        bench.iter_batched(
            || (store(), batch.clone()),
            |(mut store, batch)| {
                store.write_batch(black_box(&batch)).expect("batch writes");
                black_box(store);
            },
            BatchSize::SmallInput,
        );
    });
    group.finish();
}

criterion_group!(benches, write_batch_benches);
criterion_main!(benches);
