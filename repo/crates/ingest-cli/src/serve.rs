//! `ingest serve` — the **recon read-model service**.
//!
//! This is the query side of the CQRS seam: `ingest store` materialises the
//! normalized statements/positions as Parquet (the write side), and this
//! service answers `GET /recon/snapshot` by running a **DuckDB query over that
//! Parquet store** on every request — so the desk always reflects the current
//! store, and the data really is *queried*, not shipped as a file.
//!
//! The workbench Reconciliation desk fetches this endpoint (via the UI proxy);
//! the static snapshot file remains the offline fallback.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use axum::{extract::State, http::StatusCode, response::IntoResponse, routing::get, Json, Router};
use duckdb::Connection;

use crate::csdr::CsdrSnapshot;
use crate::snapshot::{ReconSnapshot, SnapshotEntry, SnapshotPosition, SnapshotStatement};
use ingest_penalty::{PenaltyAccrual, ReportedPenalty};

struct AppState {
    store: PathBuf,
}

pub fn run_serve(store: PathBuf, bind: SocketAddr) -> Result<()> {
    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build()?;
    rt.block_on(serve(store, bind))
}

async fn serve(store: PathBuf, bind: SocketAddr) -> Result<()> {
    let state = Arc::new(AppState { store: store.clone() });
    let app = Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .route("/recon/snapshot", get(snapshot_handler))
        .route("/csdr/snapshot", get(csdr_handler))
        .with_state(state);
    let listener = tokio::net::TcpListener::bind(bind)
        .await
        .with_context(|| format!("binding {bind}"))?;
    println!("recon read-model service on http://{bind}  (store: {})", store.display());
    println!("  GET /recon/snapshot   GET /csdr/snapshot   GET /healthz");
    axum::serve(listener, app).await?;
    Ok(())
}

async fn snapshot_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    // DuckDB is blocking; keep it off the async worker.
    let store = state.store.clone();
    match tokio::task::spawn_blocking(move || read_snapshot(&store)).await {
        Ok(Ok(snap)) => (StatusCode::OK, Json(snap)).into_response(),
        Ok(Err(e)) => (StatusCode::INTERNAL_SERVER_ERROR, format!("{e:#}")).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("join error: {e}")).into_response(),
    }
}

/// Query the Parquet store into a snapshot. A fresh in-memory DuckDB per call
/// keeps the read stateless and always current; missing tables → empty parts.
pub fn read_snapshot(store: &Path) -> Result<ReconSnapshot> {
    let conn = Connection::open_in_memory().context("opening DuckDB")?;
    let statements = read_statements(&conn, store)?;
    let positions = read_positions(&conn, store)?;
    Ok(ReconSnapshot::from_parts(statements, positions))
}

async fn csdr_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let store = state.store.clone();
    match tokio::task::spawn_blocking(move || read_csdr_snapshot(&store)).await {
        Ok(Ok(snap)) => (StatusCode::OK, Json(snap)).into_response(),
        Ok(Err(e)) => (StatusCode::INTERNAL_SERVER_ERROR, format!("{e:#}")).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("join error: {e}")).into_response(),
    }
}

/// Query the penalty Parquet tables and run the pure reconciliation into a CSDR
/// snapshot. Missing tables → empty accruals/reported → an empty, reconciled
/// snapshot (graceful, same as the recon read-model).
pub fn read_csdr_snapshot(store: &Path) -> Result<CsdrSnapshot> {
    let conn = Connection::open_in_memory().context("opening DuckDB")?;
    let accruals = read_penalty_accruals(&conn, store)?;
    let reported = read_reported_penalties(&conn, store)?;
    Ok(CsdrSnapshot::from_penalties(accruals, reported))
}

fn read_penalty_accruals(conn: &Connection, store: &Path) -> Result<Vec<PenaltyAccrual>> {
    let Some(path) = table(store, "penalty_accruals.parquet") else {
        return Ok(Vec::new());
    };
    let mut q = conn.prepare(&format!(
        "SELECT source, transaction_ref, isin, instrument_desc, instrument_type, counterparty_bic, \
                currency, quantity, reference_amount, penalty_type, penalty_rate_bps, status, \
                intended_settlement_date, business_days_failed, computed_amount, direction \
         FROM read_parquet('{path}')"
    ))?;
    let rows = q.query_map([], |r| {
        Ok(PenaltyAccrual {
            source: r.get::<_, String>(0)?,
            transaction_ref: r.get::<_, String>(1)?,
            isin: r.get::<_, String>(2)?,
            instrument_desc: r.get::<_, Option<String>>(3)?,
            instrument_type: r.get::<_, String>(4)?,
            counterparty_bic: r.get::<_, Option<String>>(5)?,
            currency: r.get::<_, String>(6)?,
            quantity: r.get::<_, Option<f64>>(7)?,
            reference_amount: r.get::<_, f64>(8)?,
            penalty_type: r.get::<_, String>(9)?,
            penalty_rate_bps: r.get::<_, f64>(10)?,
            status: r.get::<_, String>(11)?,
            intended_settlement_date: r.get::<_, Option<String>>(12)?,
            business_days_failed: r.get::<_, u32>(13)?,
            computed_amount: r.get::<_, f64>(14)?,
            direction: r.get::<_, String>(15)?,
        })
    })?;
    rows.map(|r| r.map_err(Into::into)).collect()
}

fn read_reported_penalties(conn: &Connection, store: &Path) -> Result<Vec<ReportedPenalty>> {
    let Some(path) = table(store, "penalty_statements.parquet") else {
        return Ok(Vec::new());
    };
    let mut q = conn.prepare(&format!(
        "SELECT penalty_ref, transaction_ref, isin, counterparty_bic, currency, penalty_type, \
                period, reported_amount, direction \
         FROM read_parquet('{path}')"
    ))?;
    let rows = q.query_map([], |r| {
        Ok(ReportedPenalty {
            penalty_ref: r.get::<_, String>(0)?,
            transaction_ref: r.get::<_, String>(1)?,
            isin: r.get::<_, String>(2)?,
            counterparty_bic: r.get::<_, Option<String>>(3)?,
            currency: r.get::<_, String>(4)?,
            penalty_type: r.get::<_, String>(5)?,
            period: r.get::<_, Option<String>>(6)?,
            reported_amount: r.get::<_, f64>(7)?,
            direction: r.get::<_, String>(8)?,
        })
    })?;
    rows.map(|r| r.map_err(Into::into)).collect()
}

/// A store file path escaped for a DuckDB single-quoted string literal, or
/// `None` when the file isn't present yet.
fn table(store: &Path, name: &str) -> Option<String> {
    let p = store.join(name);
    p.is_file().then(|| p.to_string_lossy().replace('\'', "''"))
}

fn read_statements(conn: &Connection, store: &Path) -> Result<Vec<SnapshotStatement>> {
    let Some(hdr) = table(store, "cash_statements.parquet") else {
        return Ok(Vec::new());
    };

    let mut headers = conn.prepare(&format!(
        "SELECT source, message_id, message_type, account, currency, opening, closing \
         FROM read_parquet('{hdr}')"
    ))?;
    let mut statements: Vec<SnapshotStatement> = Vec::new();
    let mut index: HashMap<(String, String), usize> = HashMap::new();
    let rows = headers.query_map([], |r| {
        Ok(SnapshotStatement {
            source: r.get::<_, String>(0)?,
            message_id: r.get::<_, String>(1)?,
            message_type: r.get::<_, String>(2)?,
            account: r.get::<_, Option<String>>(3)?.unwrap_or_default(),
            currency: r.get::<_, Option<String>>(4)?.unwrap_or_default(),
            opening: r.get::<_, Option<f64>>(5)?.unwrap_or(0.0),
            closing: r.get::<_, Option<f64>>(6)?.unwrap_or(0.0),
            entries: Vec::new(),
        })
    })?;
    for row in rows {
        let s = row?;
        index.insert((s.source.clone(), s.message_id.clone()), statements.len());
        statements.push(s);
    }

    // Attach entry lines to their statement header (join on source + message_id).
    if let Some(ent) = table(store, "cash_entries.parquet") {
        let mut lines = conn.prepare(&format!(
            "SELECT source, message_id, coalesce(value_date, entry_date), direction, amount, \
                    signed_amount, transaction_type, coalesce(customer_ref, bank_ref), info \
             FROM read_parquet('{ent}')"
        ))?;
        let rows = lines.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                SnapshotEntry {
                    value_date: r.get::<_, Option<String>>(2)?.unwrap_or_default(),
                    direction: r.get::<_, String>(3)?,
                    amount: r.get::<_, f64>(4)?,
                    signed_amount: r.get::<_, f64>(5)?,
                    transaction_type: r.get::<_, Option<String>>(6)?.unwrap_or_default(),
                    reference: r.get::<_, Option<String>>(7)?,
                    info: r.get::<_, Option<String>>(8)?,
                },
            ))
        })?;
        for row in rows {
            let (source, message_id, entry) = row?;
            if let Some(&i) = index.get(&(source, message_id)) {
                statements[i].entries.push(entry);
            }
        }
    }

    Ok(statements)
}

fn read_positions(conn: &Connection, store: &Path) -> Result<Vec<SnapshotPosition>> {
    let Some(pos) = table(store, "positions.parquet") else {
        return Ok(Vec::new());
    };
    let mut q = conn.prepare(&format!(
        "SELECT source, message_type, isin, instrument_desc, quantity, safekeeping_account \
         FROM read_parquet('{pos}') WHERE isin IS NOT NULL"
    ))?;
    let rows = q.query_map([], |r| {
        Ok(SnapshotPosition {
            source: r.get::<_, String>(0)?,
            message_type: r.get::<_, String>(1)?,
            isin: r.get::<_, Option<String>>(2)?.unwrap_or_default(),
            desc: r.get::<_, Option<String>>(3)?,
            quantity: r.get::<_, Option<f64>>(4)?.unwrap_or(0.0),
            safekeeping_account: r.get::<_, Option<String>>(5)?,
        })
    })?;
    rows.map(|r| r.map_err(Into::into)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ingest_core::{Balance, CashEntry, CashStatement, Direction, EventKind, SecurityEvent};

    #[test]
    fn queries_a_written_store_back_into_a_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        let store = dir.path();

        let stmt = CashStatement {
            source: "swift".into(),
            message_id: "S1".into(),
            message_type: "MT940".into(),
            account: Some("GB29".into()),
            currency: Some("EUR".into()),
            opening_balance: Some(Balance { direction: Direction::Credit, amount: 100_000.0, ..Default::default() }),
            closing_balance: Some(Balance { direction: Direction::Credit, amount: 120_000.0, ..Default::default() }),
            entries: vec![CashEntry {
                value_date: Some("2026-05-13".into()),
                direction: Direction::Credit,
                amount: 20_000.0,
                signed_amount: 20_000.0,
                transaction_type: Some("TRF".into()),
                customer_ref: Some("REF1".into()),
                ..Default::default()
            }],
            ..Default::default()
        };
        let pos = SecurityEvent {
            source: "swift".into(),
            message_type: "MT535".into(),
            kind: EventKind::Holding,
            isin: Some("GB00B03MLX29".into()),
            quantity: Some(1000.0),
            ..Default::default()
        };

        ingest_parquet::write_cash_statements(std::slice::from_ref(&stmt), &store.join("cash_statements.parquet")).unwrap();
        ingest_parquet::write_cash_entries(std::slice::from_ref(&stmt), &store.join("cash_entries.parquet")).unwrap();
        ingest_parquet::write_security_events(std::slice::from_ref(&pos), &store.join("positions.parquet")).unwrap();

        let snap = read_snapshot(store).unwrap();
        assert_eq!(snap.statements.len(), 1);
        let s = &snap.statements[0];
        assert_eq!(s.opening, 100_000.0);
        assert_eq!(s.closing, 120_000.0);
        assert_eq!(s.entries.len(), 1, "entry line joined to its header");
        assert_eq!(s.entries[0].reference.as_deref(), Some("REF1"));
        assert_eq!(snap.positions.len(), 1);
        assert_eq!(snap.positions[0].isin, "GB00B03MLX29");
    }

    #[test]
    fn empty_store_is_an_empty_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        let snap = read_snapshot(dir.path()).unwrap();
        assert!(snap.statements.is_empty());
        assert!(snap.positions.is_empty());
    }
}
