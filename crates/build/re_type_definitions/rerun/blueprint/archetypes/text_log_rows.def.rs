// This is a Rerun type definition for the SDK, not executable code.
// It is parsed by `re_types_builder` to generate the Rust, Python and C++ bindings.

/// Configuration of the text log rows.
#[rerun::rerun_type]
#[rerun(scope = "blueprint")]
#[rerun(state = "unstable")]
pub struct TextLogRows {
    /// Log levels to display.
    ///
    /// Defaults to showing all logged levels.
    #[rerun(optional)]
    pub filter_by_log_level: Option<Vec<rerun::components::TextLogLevel>>,

    /// Filters on the text log columns, all of which a row must pass to be shown.
    ///
    /// SQL boolean expressions over the `"Entity path"`, `"Level"`, and `"Body"` columns,
    /// in the same form the table view uses, e.g. `"Body" ILIKE '%timeout%'`.
    /// Defaults to no filters.
    #[rerun(optional)]
    pub filters: Option<Vec<rerun::blueprint::components::SqlFilterExpression>>,
}
