//! Cell editing rules shared by the native clients, mirroring
//! `apps/desktop/ui/src/cellValues.ts`.

use serde_json::Value;

use crate::models::TableColumn;

const TRUE_TEXT: [&str; 4] = ["true", "t", "1", "yes"];
const FALSE_TEXT: [&str; 4] = ["false", "f", "0", "no"];

/// The text shown in an editor for a cell. NULL edits as an empty field.
pub fn editable_text(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

/// Turns editor text back into a cell value for the column's type.
///
/// Text that still matches the original keeps the original, so opening and
/// closing an editor never stages a change. An empty field is NULL when the
/// column allows it. Only numeric and boolean columns get numbers and booleans;
/// other columns keep the exact text (`007` stays `007`). Decimals stay text so
/// numeric columns keep their exact digits.
pub fn parse_cell_input(text: &str, column: &TableColumn, original: &Value) -> Value {
    if text == editable_text(original) {
        return original.clone();
    }
    if text.is_empty() {
        return if column.nullable {
            Value::Null
        } else {
            Value::String(String::new())
        };
    }
    let trimmed = text.trim();
    let normalized = trimmed.to_lowercase();
    let data_type = column.data_type.to_lowercase();
    if data_type.starts_with("bool") {
        if TRUE_TEXT.contains(&normalized.as_str()) {
            return Value::Bool(true);
        }
        if FALSE_TEXT.contains(&normalized.as_str()) {
            return Value::Bool(false);
        }
        return Value::String(text.into());
    }
    let numeric = [
        "int", "serial", "numeric", "decimal", "real", "double", "float", "number",
    ]
    .iter()
    .any(|name| data_type.contains(name));
    if numeric {
        if normalized == "true" || normalized == "false" {
            return Value::Bool(normalized == "true");
        }
        if let Ok(integer) = trimmed.parse::<i64>() {
            return Value::from(integer);
        }
        return Value::String(trimmed.into());
    }
    Value::String(text.into())
}

/// Numeric columns right-align in grids so digits line up.
pub fn numeric_column(data_type: &str) -> bool {
    let lower = data_type.to_ascii_lowercase();
    let word = lower
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .next()
        .unwrap_or("");
    let with_digit = |base: &str| {
        word == base
            || word
                .strip_prefix(base)
                .is_some_and(|rest| rest.len() == 1 && rest.as_bytes()[0].is_ascii_digit())
    };
    matches!(
        word,
        "smallint"
            | "integer"
            | "bigint"
            | "tinyint"
            | "mediumint"
            | "bigserial"
            | "smallserial"
            | "numeric"
            | "decimal"
            | "real"
            | "double"
            | "money"
    ) || with_digit("int")
        || with_digit("serial")
        || with_digit("float")
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn column(data_type: &str, nullable: bool) -> TableColumn {
        TableColumn {
            name: "c".into(),
            data_type: data_type.into(),
            nullable,
            default_value: None,
            ordinal: 1,
        }
    }

    #[test]
    fn unchanged_text_keeps_the_original_value() {
        let big = json!("9007199254740993");
        assert_eq!(
            parse_cell_input("9007199254740993", &column("bigint", false), &big),
            big
        );
        assert_eq!(
            parse_cell_input("", &column("text", true), &Value::Null),
            Value::Null
        );
        assert_eq!(editable_text(&json!({"a": 1})), "{\"a\":1}");
    }

    #[test]
    fn empty_text_is_null_only_for_nullable_columns() {
        assert_eq!(
            parse_cell_input("", &column("text", true), &json!("x")),
            Value::Null
        );
        assert_eq!(
            parse_cell_input("", &column("text", false), &json!("x")),
            json!("")
        );
        assert_eq!(
            parse_cell_input("NULL", &column("text", true), &json!("x")),
            json!("NULL")
        );
    }

    #[test]
    fn typed_columns_parse_numbers_and_booleans() {
        let bool_col = column("boolean", false);
        assert_eq!(
            parse_cell_input("yes", &bool_col, &json!(false)),
            json!(true)
        );
        assert_eq!(parse_cell_input("F", &bool_col, &json!(true)), json!(false));
        assert_eq!(
            parse_cell_input("maybe", &bool_col, &json!(true)),
            json!("maybe")
        );
        let int_col = column("integer", false);
        assert_eq!(parse_cell_input(" 42 ", &int_col, &json!(1)), json!(42));
        assert_eq!(
            parse_cell_input("1.50", &column("numeric(10,2)", false), &json!("1")),
            json!("1.50")
        );
        assert_eq!(
            parse_cell_input("007", &column("text", false), &json!("7")),
            json!("007")
        );
    }

    #[test]
    fn numeric_columns_right_align() {
        for data_type in [
            "bigint",
            "int4",
            "INTEGER",
            "numeric(10,2)",
            "double precision",
            "float8",
            "money",
        ] {
            assert!(numeric_column(data_type), "{data_type}");
        }
        for data_type in ["text", "interval", "point", "int45", "character varying"] {
            assert!(!numeric_column(data_type), "{data_type}");
        }
    }
}
