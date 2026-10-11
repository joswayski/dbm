//! Full-table CSV export shared by the native hosts, matching the desktop
//! app's file format: a UTF-8 byte-order mark for spreadsheet apps, the
//! header, then every row matching the current filters and order.

use std::future::Future;
use std::io::Write;
use std::path::Path;

use serde_json::Value;

use crate::error::{AppError, AppResult};
use crate::models::{TablePage, TablePageRequest};
use crate::sql_text::csv_line;
use crate::state::AppState;

/// Rows fetched per request while exporting a paged source (Redis and the
/// demo fixture), as in the desktop app.
pub const EXPORT_PAGE_SIZE: u32 = 1_000;

/// Receives each exported row; an error stops the export.
pub type RowSink<'a> = dyn FnMut(Vec<Value>) -> AppResult<()> + Send + 'a;

/// A CSV export being written to a temporary file next to its destination,
/// which is replaced only by `finish`. Dropping it unfinished (after a failure
/// or cancellation) removes the temporary file and leaves the destination as
/// it was.
pub struct CsvExport {
    out: std::io::BufWriter<tempfile::NamedTempFile>,
    columns: usize,
    written: u64,
}

impl CsvExport {
    /// Starts the file with a UTF-8 byte-order mark for spreadsheet apps and
    /// the header row.
    pub fn create(path: &Path, columns: &[String]) -> Result<Self, String> {
        // Keep the temporary file on the destination filesystem for an atomic
        // replacement.
        let parent = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let file = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
        let mut out = std::io::BufWriter::new(file);
        let header: Vec<Value> = columns.iter().cloned().map(Value::String).collect();
        write!(out, "\u{feff}{}", csv_line(&header)).map_err(|e| e.to_string())?;
        Ok(Self {
            out,
            columns: columns.len(),
            written: 0,
        })
    }

    /// Writes one row, ignoring values beyond the header's columns (such as
    /// PostgreSQL's `xmin`).
    pub fn write_row(&mut self, row: &[Value]) -> Result<(), String> {
        write!(self.out, "\n{}", csv_line(row.iter().take(self.columns)))
            .map_err(|e| e.to_string())?;
        self.written += 1;
        Ok(())
    }

    pub fn written(&self) -> u64 {
        self.written
    }

    /// Flushes the file and moves it over `path`; returns the rows written.
    pub fn finish(self, path: &Path) -> Result<u64, String> {
        let file = self.out.into_inner().map_err(|e| e.to_string())?;
        file.as_file().sync_all().map_err(|e| e.to_string())?;
        file.persist(path).map_err(|e| e.to_string())?;
        Ok(self.written)
    }
}

/// Saves an already-loaded result without executing its query again. Like a
/// table export, this replaces the destination only after writing succeeds.
pub fn export_result_csv(
    path: &Path,
    columns: &[String],
    rows: &[Vec<Value>],
) -> Result<u64, String> {
    let mut file = CsvExport::create(path, columns)?;
    for row in rows {
        file.write_row(row)?;
    }
    file.finish(path)
}

/// Exports every row matching `request`'s filters and order (its offset and
/// limit are ignored) from the profile's live session to `path` as CSV, and
/// returns the number of rows written. PostgreSQL and MySQL stream one
/// statement, so tables without a primary key export each row exactly once;
/// Redis pages in key order. `progress` receives the rows written so far. A
/// connection lost before the export finishes is reconnected and the export
/// restarts from the first row.
pub async fn export_table(
    state: &AppState,
    path: &Path,
    columns: &[String],
    request: &TablePageRequest,
    progress: &(dyn Fn(u64) + Sync),
) -> Result<u64, String> {
    let export = |session: std::sync::Arc<crate::session::DbSession>| async move {
        progress(0);
        let mut file = CsvExport::create(path, columns).map_err(AppError::Storage)?;
        let mut sink = |row: Vec<Value>| {
            file.write_row(&row).map_err(AppError::Storage)?;
            // Report every 100 rows rather than on each one.
            if file.written() % 100 == 0 {
                progress(file.written());
            }
            Ok(())
        };
        session.export_rows(request, &mut sink).await?;
        file.finish(path).map_err(AppError::Storage)
    };
    state
        .with_session_retry(request.profile_id, export)
        .await
        .map_err(|error| match error {
            AppError::Storage(message) => message,
            other => other.to_string(),
        })
}

/// Writes every page of `request` (its offset and limit are ignored) to
/// `path` as CSV and returns the number of rows written. Used for paged
/// sources such as the demo fixture. The destination is replaced only after
/// the complete export succeeds; failures leave it intact.
pub async fn export_csv<F, Fut, E>(
    path: &Path,
    columns: &[String],
    request: &TablePageRequest,
    mut load: F,
) -> Result<u64, String>
where
    F: FnMut(TablePageRequest) -> Fut,
    Fut: Future<Output = Result<TablePage, E>>,
    E: std::fmt::Display,
{
    let mut file = CsvExport::create(path, columns)?;
    let mut offset = 0u32;
    loop {
        let page = load(TablePageRequest {
            offset,
            limit: EXPORT_PAGE_SIZE,
            include_total: Some(false),
            ..request.clone()
        })
        .await
        .map_err(|e| e.to_string())?;
        for row in &page.rows {
            file.write_row(row)?;
        }
        offset += u32::try_from(page.rows.len()).map_err(|e| e.to_string())?;
        if !page.has_more || page.rows.is_empty() {
            break;
        }
    }
    file.finish(path)
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use uuid::Uuid;

    use super::*;
    use crate::models::{TableColumn, TableMetadata};

    fn page(request: &TablePageRequest, total: u32) -> TablePage {
        let rows: Vec<_> = (request.offset..total.min(request.offset + request.limit))
            .map(|i| vec![json!(i), json!(format!("a,{i}")), json!("xmin")])
            .collect();
        TablePage {
            metadata: TableMetadata {
                schema: "s".into(),
                table: "t".into(),
                columns: ["id", "note"]
                    .iter()
                    .enumerate()
                    .map(|(i, name)| TableColumn {
                        name: (*name).into(),
                        data_type: "text".into(),
                        nullable: true,
                        default_value: None,
                        ordinal: i as i32 + 1,
                    })
                    .collect(),
                primary_key: vec!["id".into()],
                has_xmin: true,
            },
            columns: vec!["id".into(), "note".into(), "__dbm_xmin".into()],
            rows,
            total_rows: None,
            offset: request.offset,
            limit: request.limit,
            has_more: request.offset + request.limit < total,
        }
    }

    fn request() -> TablePageRequest {
        TablePageRequest {
            profile_id: Uuid::nil(),
            schema: "s".into(),
            table: "t".into(),
            offset: 40,
            limit: 5,
            filters: vec![],
            order_by: None,
            include_total: Some(true),
        }
    }

    fn run<T>(future: impl Future<Output = T>) -> T {
        tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap()
            .block_on(future)
    }

    #[test]
    fn result_export_preserves_columns_values_and_empty_result_headers() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("query.csv");
        let columns = vec!["customer,name".into(), "note".into(), "active".into()];
        let rows = vec![
            vec![json!("José"), json!("a\"b\nnext"), json!(true)],
            vec![json!("Z"), json!(null), json!(false)],
        ];
        std::fs::write(&path, "old export").unwrap();
        assert_eq!(export_result_csv(&path, &columns, &rows).unwrap(), 2);
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "\u{feff}\"customer,name\",note,active\nJosé,\"a\"\"b\nnext\",true\nZ,NULL,false"
        );
        assert_eq!(export_result_csv(&path, &columns, &[]).unwrap(), 0);
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "\u{feff}\"customer,name\",note,active"
        );
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
    }

    #[test]
    fn writes_bom_header_and_every_page() {
        let path = std::env::temp_dir().join(format!("dbm-export-{}.csv", Uuid::new_v4()));
        let columns = vec!["id".to_owned(), "note".to_owned()];
        let mut calls = Vec::new();
        let rows = run(export_csv(&path, &columns, &request(), |req| {
            calls.push((req.offset, req.limit, req.include_total));
            let page = page(&req, 2_500);
            async move { Ok::<_, String>(page) }
        }))
        .unwrap();
        assert_eq!(rows, 2_500);
        assert_eq!(
            calls,
            vec![
                (0, 1_000, Some(false)),
                (1_000, 1_000, Some(false)),
                (2_000, 1_000, Some(false))
            ]
        );
        let text = std::fs::read_to_string(&path).unwrap();
        let mut lines = text.lines();
        assert_eq!(lines.next(), Some("\u{feff}id,note"));
        assert_eq!(lines.next(), Some("0,\"a,0\""));
        assert_eq!(text.lines().count(), 2_501);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn failures_remove_the_partial_file() {
        let path = std::env::temp_dir().join(format!("dbm-export-{}.csv", Uuid::new_v4()));
        let error = run(export_csv(
            &path,
            &["id".to_owned()],
            &request(),
            |_| async { Err::<TablePage, _>("connection lost") },
        ))
        .unwrap_err();
        assert_eq!(error, "connection lost");
        assert!(!path.exists());
    }

    #[test]
    fn failed_exports_preserve_existing_files_and_remove_temporary_files() {
        for failed_page in [0, EXPORT_PAGE_SIZE] {
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().join("existing.csv");
            std::fs::write(&path, "previous export").unwrap();
            let error = run(export_csv(&path, &["id".into()], &request(), |req| {
                let result = if req.offset == failed_page {
                    Err("connection lost")
                } else {
                    Ok(page(&req, 2_500))
                };
                async move { result }
            }))
            .unwrap_err();
            assert_eq!(error, "connection lost");
            assert_eq!(std::fs::read_to_string(&path).unwrap(), "previous export");
            assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
        }
    }

    #[test]
    fn successful_export_replaces_the_destination_only_after_all_pages_load() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("existing.csv");
        std::fs::write(&path, "previous export").unwrap();
        let rows = run(export_csv(&path, &["id".into()], &request(), |req| {
            assert_eq!(std::fs::read_to_string(&path).unwrap(), "previous export");
            let page = page(&req, 1_001);
            async move { Ok::<_, String>(page) }
        }))
        .unwrap();
        assert_eq!(rows, 1_001);
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.starts_with("\u{feff}id\n0\n1\n"));
        assert!(text.ends_with("\n999\n1000"));
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
    }

    #[test]
    fn failed_destination_replacement_removes_temporary_files() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("destination.csv");
        std::fs::create_dir(&path).unwrap();
        std::fs::write(path.join("keep"), "untouched").unwrap();
        assert!(
            run(export_csv(&path, &["id".into()], &request(), |req| {
                let page = page(&req, 1);
                async move { Ok::<_, String>(page) }
            }))
            .is_err()
        );
        assert_eq!(
            std::fs::read_to_string(path.join("keep")).unwrap(),
            "untouched"
        );
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
    }

    /// Live checks of `export_table` against `DBM_TEST_POSTGRES_PORT` and
    /// `DBM_TEST_MYSQL_PORT` (local servers trusting `postgres` / `root`).
    #[test]
    fn tables_without_a_primary_key_export_every_row_exactly_once() {
        use crate::models::{
            ConnectionProfile, DatabaseEngine, FilterCondition, FilterOperator, TlsMode,
        };
        use std::sync::atomic::{AtomicU64, Ordering};

        for (engine, variable, username, database) in [
            (
                DatabaseEngine::Postgres,
                "DBM_TEST_POSTGRES_PORT",
                "postgres",
                "postgres",
            ),
            (
                DatabaseEngine::Mysql,
                "DBM_TEST_MYSQL_PORT",
                "root",
                "mysql",
            ),
        ] {
            let Some(port) = std::env::var(variable).ok().and_then(|p| p.parse().ok()) else {
                continue;
            };
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            runtime.block_on(async {
                let directory = tempfile::tempdir().unwrap();
                let state = AppState::mobile(&directory.path().join("state.sqlite3")).unwrap();
                let id = Uuid::new_v4();
                let profile = ConnectionProfile {
                    id,
                    name: "test-export".into(),
                    color: None,
                    engine,
                    host: "127.0.0.1".into(),
                    port,
                    username: username.into(),
                    default_database: database.into(),
                    tls_mode: TlsMode::Disabled,
                    ca_cert_path: None,
                    ssh: None,
                    read_only: false,
                    open_on_startup: false,
                    created_at: chrono::Utc::now(),
                    updated_at: chrono::Utc::now(),
                };
                state.connect(profile).await.unwrap();
                let session = state.session(id).await.unwrap();
                let table = format!("dbm_export_{}", id.simple());
                // No primary key, duplicate rows, and more rows than a page.
                let (schema, create, fill) = match engine {
                    DatabaseEngine::Postgres => (
                        "public".to_owned(),
                        format!("CREATE TABLE public.{table} (n int, label text)"),
                        format!(
                            "INSERT INTO public.{table} SELECT g % 1200, 'row ' || (g % 1200) \
                             FROM generate_series(1, 2500) g"
                        ),
                    ),
                    _ => (
                        table.clone(),
                        format!(
                            "CREATE DATABASE {table}; CREATE TABLE {table}.{table} (n int, label text)"
                        ),
                        format!(
                            "INSERT INTO {table}.{table} \
                             WITH RECURSIVE g(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM g WHERE i < 2500) \
                             SELECT i % 1200, CONCAT('row ', i % 1200) FROM g"
                        ),
                    ),
                };
                session.run_query(&create, None).await.unwrap();
                if engine == DatabaseEngine::Mysql {
                    // MySQL and MariaDB name the recursion limit differently.
                    for limit in ["cte_max_recursion_depth", "max_recursive_iterations"] {
                        let set = format!("SET SESSION {limit} = 3000");
                        session.run_query(&set, None).await.ok();
                    }
                }
                session.run_query(&fill, None).await.unwrap();
                let path = directory.path().join("export.csv");
                let progress = AtomicU64::new(0);
                let report = |rows: u64| progress.store(rows, Ordering::Relaxed);
                let request = TablePageRequest {
                    profile_id: id,
                    schema: schema.clone(),
                    table: table.clone(),
                    offset: 40,
                    limit: 5,
                    filters: vec![FilterCondition {
                        column: "n".into(),
                        operator: FilterOperator::LessThan,
                        value: Some("1000".into()),
                    }],
                    order_by: None,
                    include_total: Some(true),
                };
                let columns = vec!["n".to_owned(), "label".to_owned()];
                let rows = export_table(&state, &path, &columns, &request, &report)
                    .await
                    .unwrap();
                let drop = match engine {
                    DatabaseEngine::Postgres => format!("DROP TABLE public.{table}"),
                    _ => format!("DROP DATABASE {table}"),
                };
                session.run_query(&drop, None).await.unwrap();
                // Values repeat (g % 1200), so the table has duplicate rows.
                let expected = (1..=2500).filter(|g| g % 1200 < 1000).count();
                assert_eq!(rows as usize, expected, "{engine:?}");
                assert_eq!(
                    progress.load(Ordering::Relaxed) as usize / 100,
                    expected / 100
                );
                let text = std::fs::read_to_string(&path).unwrap();
                let mut lines: Vec<&str> = text.lines().skip(1).collect();
                lines.sort_unstable();
                let mut wanted: Vec<String> = (1..=2500)
                    .map(|g| g % 1200)
                    .filter(|n| *n < 1000)
                    .map(|n| format!("{n},row {n}"))
                    .collect();
                wanted.sort_unstable();
                assert_eq!(lines, wanted, "{engine:?}: each row exactly once");
            });
        }
    }
}
