use criterion::{black_box, criterion_group, criterion_main, Criterion};
use std::alloc::{GlobalAlloc, Layout, System};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use swift_schema::{render_message, RenderEnvelope, RenderRequest, RenderRow, SchemaCatalog};

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);

struct CountingAllocator;

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) };
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

const SCHEMA: &str = r#"
field_types:
  - name: reference
    pattern:
      - kind: literal
        value: ":"
      - kind: capture
        name: qualifier
        value_type: alpha_num
        length: 4
      - kind: literal
        value: "//"
      - kind: rest
        name: value
  - name: date_yyyymmdd
    pattern:
      - kind: literal
        value: ":"
      - kind: capture
        name: qualifier
        value_type: alpha_num
        length: 4
      - kind: literal
        value: "//"
      - kind: capture
        name: date
        value_type: digits
        length: 8
messages:
  - message: MT540
    category: securities
    version: "2026"
    sequences:
      GENL:
        repeat: false
      LINK:
        parent: GENL
        repeat: true
    fields:
      - path: GENL
        tag: 20C
        qualifier: SEME
        name: sender_reference
        type: reference
        required: true
        entity: settlement_instruction
        column: sender_reference
      - path: GENL
        tag: 98A
        qualifier: PREP
        name: preparation_date
        type: date_yyyymmdd
        entity: settlement_instruction
        column: preparation_date
      - path: GENL/LINK
        tag: 20C
        qualifier: RELA
        name: related_reference
        type: reference
        entity: message_reference
        column: related_reference
"#;

fn request() -> (SchemaCatalog, RenderRequest) {
    let catalog = SchemaCatalog::from_yaml_str(SCHEMA).expect("schema loads");
    catalog.validate_rendering().expect("schema renders");
    let rows = vec![
        render_row(
            "settlement_instruction",
            "GENL[1]",
            "sender_reference",
            "ABC123",
        ),
        render_row(
            "settlement_instruction",
            "GENL[1]",
            "preparation_date",
            "2026-05-11",
        ),
        render_row(
            "message_reference",
            "GENL[1]/LINK[1]",
            "related_reference",
            "REL1",
        ),
    ];
    (
        catalog,
        RenderRequest {
            message_id: "msg-1".to_string(),
            message_type: "MT540".to_string(),
            envelope: RenderEnvelope {
                block1: "F01BANKBEBBAXXX0000000000".to_string(),
                block2: "I540BANKDEFFXXXXN".to_string(),
                block3: None,
                block5: None,
            },
            rows,
        },
    )
}

fn render_row(table: &str, sequence_path: &str, column: &str, value: &str) -> RenderRow {
    RenderRow {
        table: table.to_string(),
        values: BTreeMap::from([
            ("message_id".to_string(), "msg-1".to_string()),
            ("sequence_path".to_string(), sequence_path.to_string()),
            (column.to_string(), value.to_string()),
        ]),
    }
}

fn allocation_count_for(catalog: &SchemaCatalog, request: &RenderRequest) -> usize {
    ALLOCATIONS.store(0, Ordering::Relaxed);
    let rendered = render_message(catalog, request).expect("message renders");
    assert!(rendered.contains(":20C::SEME//ABC123"));
    let allocations = ALLOCATIONS.load(Ordering::Relaxed);
    black_box(rendered);
    allocations
}

fn render_allocation_benches(c: &mut Criterion) {
    let (catalog, request) = request();
    let allocations = allocation_count_for(&catalog, &request);
    eprintln!("swift_schema_render_allocations/mt540_three_fields allocations: {allocations}");
    assert!(allocations <= 128);

    let mut group = c.benchmark_group("swift_schema_render_allocations");
    group.bench_function("mt540_three_fields", |bench| {
        bench.iter(|| {
            black_box(allocation_count_for(
                black_box(&catalog),
                black_box(&request),
            ))
        });
    });
    group.finish();
}

criterion_group!(benches, render_allocation_benches);
criterion_main!(benches);
