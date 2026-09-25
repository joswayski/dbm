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
/// `path` as CSV and returns the number of rows written. A partial file is
/// removed when loading or writing fails.
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
    let result = write(path, columns, request, &mut load).await;
    if result.is_err() {
        let _ = std::fs::remove_file(path);
    }
    result
}

async fn write<F, Fut, E>(
    path: &Path,
    columns: &[String],
    request: &TablePageRequest,
    load: &mut F,
) -> Result<u64, String>
where
    F: FnMut(TablePageRequest) -> Fut,
    Fut: Future<Output = Result<TablePage, E>>,
    E: std::fmt::Display,
{
    let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
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
    out.flush().map_err(|e| e.to_string())?;
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
}
