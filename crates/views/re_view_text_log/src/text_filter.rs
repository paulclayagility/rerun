//! Filters on the text log columns, edited with the table view's filter bar.

use std::ops::Range;
use std::sync::Arc;

use arrow::datatypes::{DataType, Field, FieldRef, Schema};
use re_dataframe_ui::{ColumnFilter, StringOperator, TableFilter, TypedFilter};
use re_sdk_types::blueprint::encodings::TextLogColumnKind;

/// The columns that can be filtered on. Filters refer to them by their header name.
pub const FILTERABLE_COLUMNS: [TextLogColumnKind; 3] = [
    TextLogColumnKind::EntityPath,
    TextLogColumnKind::LogLevel,
    TextLogColumnKind::Body,
];

/// The field the filter bar uses for a column.
pub fn column_field(column: TextLogColumnKind) -> FieldRef {
    Arc::new(Field::new(column.name(), DataType::Utf8, true))
}

/// The schema the filter SQL is written against.
pub fn schema() -> Schema {
    Schema::new_with_metadata(
        FILTERABLE_COLUMNS.map(column_field).to_vec(),
        Default::default(),
    )
}

/// A string filter on one column, matched like SQL `ILIKE`, i.e. ignoring case.
struct ColumnTextFilter {
    column: TextLogColumnKind,
    operator: StringOperator,

    /// Lowercased.
    query: String,
}

/// The filters of a text log view, all of which a row must pass.
#[derive(Default)]
pub struct TextFilters {
    filters: Vec<ColumnTextFilter>,

    /// SQL of the filters this view can't evaluate. They don't filter anything.
    unsupported: Vec<String>,
}

impl TextFilters {
    pub fn from_sql(filters: &[String]) -> Self {
        let schema = schema();
        let mut text_filters = Self::default();
        for sql in filters {
            // With this schema, every column filter is a string filter on one of our columns.
            if let TableFilter::Column(ColumnFilter {
                field,
                filter: TypedFilter::String(filter),
            }) = TableFilter::from_sql(sql, &schema)
                && let Some(column) = FILTERABLE_COLUMNS
                    .into_iter()
                    .find(|column| column.name() == field.name())
            {
                text_filters.filters.push(ColumnTextFilter {
                    column,
                    operator: filter.operator(),
                    query: filter.query().to_lowercase(),
                });
            } else {
                text_filters.unsupported.push(sql.clone());
            }
        }
        text_filters
    }

    /// SQL of the filters this view can't evaluate.
    pub fn unsupported(&self) -> &[String] {
        &self.unsupported
    }

    /// Whether no filter removes any row, so row text doesn't need to be read.
    pub fn is_empty(&self) -> bool {
        self.filters.is_empty()
    }

    /// Whether a row passes all filters. Rows without a level have an empty level.
    pub fn matches(&self, entity_path: &str, level: Option<&str>, body: &str) -> bool {
        self.filters.iter().all(|filter| {
            let text = match filter.column {
                TextLogColumnKind::EntityPath => entity_path,
                TextLogColumnKind::LogLevel => level.unwrap_or(""),
                TextLogColumnKind::Body => body,
            }
            .to_lowercase();
            let query = filter.query.as_str();
            match filter.operator {
                StringOperator::Contains => text.contains(query),
                StringOperator::DoesNotContain => !text.contains(query),
                StringOperator::StartsWith => text.starts_with(query),
                StringOperator::EndsWith => text.ends_with(query),
            }
        })
    }

    /// Byte ranges of `text` matched by filters on `column`, sorted and non-overlapping.
    pub fn highlights(&self, column: TextLogColumnKind, text: &str) -> Vec<Range<usize>> {
        let lowercase = text.to_lowercase();
        if lowercase.len() != text.len() {
            // Lowercasing changed byte offsets; don't risk highlighting the wrong text.
            return Vec::new();
        }

        let mut ranges = Vec::new();
        for filter in self.filters.iter().filter(|filter| filter.column == column) {
            let query = filter.query.as_str();
            if query.is_empty() {
                continue;
            }
            match filter.operator {
                StringOperator::Contains => ranges.extend(
                    lowercase
                        .match_indices(query)
                        .map(|(start, _)| start..start + query.len()),
                ),
                StringOperator::StartsWith if lowercase.starts_with(query) => {
                    ranges.push(0..query.len());
                }
                StringOperator::EndsWith if lowercase.ends_with(query) => {
                    ranges.push(text.len() - query.len()..text.len());
                }
                _ => {}
            }
        }

        ranges.sort_by_key(|range| range.start);
        let mut merged: Vec<Range<usize>> = Vec::with_capacity(ranges.len());
        for range in ranges {
            match merged.last_mut() {
                Some(last) if range.start <= last.end => last.end = last.end.max(range.end),
                _ => merged.push(range),
            }
        }
        merged
    }
}

#[cfg(test)]
mod tests {
    use StringOperator::{Contains, DoesNotContain, EndsWith, StartsWith};
    use TextLogColumnKind::{Body, EntityPath as Path, LogLevel};
    use re_dataframe_ui::StringFilter;

    use super::*;

    fn sql(column: TextLogColumnKind, operator: StringOperator, query: &str) -> String {
        ColumnFilter::new(column_field(column), StringFilter::new(operator, query))
            .to_sql()
            .expect("non-empty query")
    }

    #[test]
    fn operators_ignore_case() {
        let matches = |column, op, query| {
            TextFilters::from_sql(&[sql(column, op, query)]).matches(
                "/log",
                Some("WARN"),
                "Request FAILED: POST /api",
            )
        };
        assert!(matches(Body, Contains, "failed"));
        assert!(!matches(Body, DoesNotContain, "failed"));
        assert!(matches(Body, StartsWith, "request"));
        assert!(matches(Body, EndsWith, "/API"));
        assert!(matches(LogLevel, Contains, "warn"));
        assert!(matches(Path, StartsWith, "/log"));
    }

    #[test]
    fn filters_combine_with_and() {
        let filters = TextFilters::from_sql(&[
            sql(Body, Contains, "failed"),
            sql(Body, DoesNotContain, "worker"),
        ]);
        assert!(filters.matches("/log", None, "request failed"));
        assert!(!filters.matches("/log", None, "worker-1 job failed"));
        assert!(!filters.matches("/log", None, "request ok"));
    }

    #[test]
    fn rows_without_level_have_an_empty_level() {
        let filters = TextFilters::from_sql(&[sql(LogLevel, DoesNotContain, "debug")]);
        assert!(filters.matches("/log", None, "x"));
    }

    #[test]
    fn unsupported_sql_is_reported_and_ignored() {
        let filters =
            TextFilters::from_sql(&[r#""Body" ~ 'regex'"#.to_owned(), "not sql (".to_owned()]);
        assert_eq!(filters.unsupported().len(), 2);
        assert!(filters.is_empty());
        assert!(filters.matches("/log", None, "anything"));
    }

    #[test]
    fn highlights_merge_and_skip_exclusions() {
        let filters = TextFilters::from_sql(&[
            sql(Body, Contains, "ab"),
            sql(Body, Contains, "bc"),
            sql(Body, DoesNotContain, "zz"),
            sql(LogLevel, Contains, "ab"),
        ]);
        assert_eq!(filters.highlights(Body, "xABCx abx"), vec![1..4, 6..8]);
        assert!(filters.highlights(Body, "nothing").is_empty());
    }
}
