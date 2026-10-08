//! Full-table CSV export shared by the native hosts, matching the desktop
//! app's file format: a UTF-8 byte-order mark for spreadsheet apps, the
//! header, then every row matching the current filters and order.

use std::future::Future;
use std::io::Write;
use std::path::Path;

use crate::models::{TablePage, TablePageRequest};
use crate::sql_text::csv_line;

/// Rows fetched per request while exporting, as in the desktop app.
pub const EXPORT_PAGE_SIZE: u32 = 1_000;

/// Streams every page of `request` (its offset and limit are ignored) to
/// `path` as CSV and returns the number of rows written. The destination is
/// replaced only after the complete export succeeds; failures leave it intact.
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
    // Keep the temporary file on the destination filesystem for an atomic
    // replacement. Dropping it also cleans up failed or canceled exports.
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let file = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
    let mut out = std::io::BufWriter::new(file);
    let header: Vec<serde_json::Value> = columns
        .iter()
        .cloned()
        .map(serde_json::Value::String)
        .collect();
    write!(out, "\u{feff}{}", csv_line(&header)).map_err(|e| e.to_string())?;
    let mut offset = 0u32;
    let mut written = 0u64;
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
            write!(out, "\n{}", csv_line(row.iter().take(columns.len())))
                .map_err(|e| e.to_string())?;
        }
        written += page.rows.len() as u64;
        offset += u32::try_from(page.rows.len()).map_err(|e| e.to_string())?;
        if !page.has_more || page.rows.is_empty() {
            break;
        }
    }
    let file = out.into_inner().map_err(|e| e.to_string())?;
    file.as_file().sync_all().map_err(|e| e.to_string())?;
    file.persist(path).map_err(|e| e.to_string())?;
    Ok(written)
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
}
