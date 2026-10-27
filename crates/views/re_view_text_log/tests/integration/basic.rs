use std::sync::Arc;

use arrow::datatypes::{DataType, Field};
use re_chunk::Chunk;
use re_dataframe_ui::{ColumnFilter, StringFilter, StringOperator};
use re_log_types::{TimeInt, Timeline};
use re_sdk_types::Archetype as _;
use re_sdk_types::archetypes::TextLog;
use re_sdk_types::blueprint::archetypes::TextLogRows;
use re_test_context::TestContext;
use re_test_context::external::egui_kittest::SnapshotResults;
use re_test_context::external::egui_kittest::kittest::Queryable as _;
use re_test_viewport::TestContextExt as _;
use re_view_text_log::TextView;
use re_viewer_context::{BlueprintContext as _, ViewClass as _, ViewId};
use re_viewport_blueprint::ViewBlueprint;

fn setup_blueprint(test_context: &mut TestContext) -> ViewId {
    test_context.setup_viewport_blueprint(|_ctx, blueprint| {
        blueprint.add_view_at_root(ViewBlueprint::new_with_root_wildcard(TextView::identifier()))
    })
}

#[test]
fn temporal_anchor_between_sequence_steps() {
    let mut snapshot_results = SnapshotResults::new();
    let mut test_context = TestContext::new_with_view_class::<TextView>();

    let timeline = Timeline::log_tick();

    let chunks = &mut text_log_chunks(timeline);

    // Only add the first one (tick = 0)
    test_context.add_chunks(chunks.take(1));
    test_context.set_active_timeline(*timeline.name());

    // The temporal anchor is intentionally between two sequence steps.
    test_context.set_time(TimeInt::new_temporal(95));

    let view_id = setup_blueprint(&mut test_context);
    snapshot_results.add(test_context.run_view_ui_and_save_snapshot(
        view_id,
        "text_log_temporal_anchor_between_steps_first_chunk",
        egui::vec2(500.0, 180.0),
        None,
    ));

    test_context.add_chunks(chunks);
    test_context.set_time(TimeInt::new_temporal(100));
    snapshot_results.add(test_context.run_view_ui_and_save_snapshot(
        view_id,
        "text_log_temporal_anchor_between_steps_rest",
        egui::vec2(500.0, 180.0),
        None,
    ));
}

/// With a level filter active, only rows with a matching level are shown.
#[test]
fn level_filter() {
    let mut snapshot_results = SnapshotResults::new();
    let mut test_context = TestContext::new_with_view_class::<TextView>();

    let timeline = Timeline::log_tick();

    let mut builder = Chunk::builder("logs");
    for tick in 0_i64..30 {
        let level = if tick % 2 == 0 { "INFO" } else { "WARN" };
        let row = TextLog::new(format!("Log at tick {tick}")).with_level(level);
        builder = builder.with_archetype_auto_row([(timeline, tick)], &row);
    }
    test_context.add_chunks(std::iter::once(
        builder.build().expect("failed to build chunk"),
    ));

    test_context.set_active_timeline(*timeline.name());
    test_context.set_time(TimeInt::new_temporal(15));

    let view_id = setup_blueprint_with_rows(
        &mut test_context,
        &TextLogRows::new().with_filter_by_log_level(["WARN"]),
    );

    snapshot_results.add(test_context.run_view_ui_and_save_snapshot(
        view_id,
        "text_log_level_filter",
        egui::vec2(500.0, 420.0),
        None,
    ));
}

/// With column filters active, only rows passing all of them are shown, ignoring case, with the
/// matches highlighted and the filters shown in the filter bar.
#[test]
fn column_filters() {
    let mut snapshot_results = SnapshotResults::new();
    let mut test_context = TestContext::new_with_view_class::<TextView>();

    let timeline = Timeline::log_tick();

    let mut builder = Chunk::builder("logs");
    for tick in 0_i64..30 {
        let body = match tick % 3 {
            0 => format!("Motor fault at tick {tick}"),
            1 => format!("motor ok at tick {tick}"),
            _ => format!("Heartbeat at tick {tick}"),
        };
        builder = builder.with_archetype_auto_row([(timeline, tick)], &TextLog::new(body));
    }
    test_context.add_chunks(std::iter::once(
        builder.build().expect("failed to build chunk"),
    ));

    test_context.set_active_timeline(*timeline.name());
    test_context.set_time(TimeInt::new_temporal(15));

    let body_filter = |operator, query| {
        let body = Arc::new(Field::new("Body", DataType::Utf8, true));
        ColumnFilter::new(body, StringFilter::new(operator, query))
            .to_sql()
            .expect("non-empty query")
    };
    let view_id = setup_blueprint_with_rows(
        &mut test_context,
        &TextLogRows::new().with_filters([
            body_filter(StringOperator::Contains, "MOTOR"),
            body_filter(StringOperator::DoesNotContain, "ok"),
        ]),
    );

    snapshot_results.add(test_context.run_view_ui_and_save_snapshot(
        view_id,
        "text_log_column_filters",
        egui::vec2(500.0, 420.0),
        None,
    ));
}

/// Right-clicking a column header opens the same menu as its "More options" button, from which
/// a filter on that column can be added.
#[test]
fn column_header_context_menu_adds_filter() {
    let mut test_context = TestContext::new_with_view_class::<TextView>();
    let timeline = Timeline::log_tick();
    test_context.add_chunks(text_log_chunks(timeline));
    test_context.set_active_timeline(*timeline.name());
    let view_id = setup_blueprint(&mut test_context);

    let mut harness = test_context
        .setup_kittest_for_rendering_ui([500.0, 300.0])
        .build_ui(|ui| test_context.run_with_single_view(ui, view_id));
    harness.run();

    // `egui::Sides` lays out the header label twice, at the same place.
    harness
        .query_all_by_label("Body")
        .next()
        .expect("Body column header")
        .click_secondary();
    harness.run();
    harness.get_by_label("Filter").click();
    harness.run();

    assert!(
        harness.query_by_label("Body contains \"\"").is_some(),
        "expected a new, empty filter on the Body column"
    );
}

fn setup_blueprint_with_rows(test_context: &mut TestContext, rows: &TextLogRows) -> ViewId {
    test_context.setup_viewport_blueprint(|ctx, blueprint| {
        let view_id = blueprint
            .add_view_at_root(ViewBlueprint::new_with_root_wildcard(TextView::identifier()));

        let property_path = {
            let engine = ctx.store_context.blueprint.storage_engine();
            re_viewport_blueprint::entity_path_for_view_property(
                view_id,
                engine.store().entity_tree(),
                TextLogRows::name(),
            )
        };
        ctx.save_blueprint_archetype(property_path, rows);

        view_id
    })
}

fn text_log_chunks(timeline: Timeline) -> impl Iterator<Item = Chunk> {
    (0_i64..=200).step_by(10).map(move |tick| {
        Chunk::builder("logs")
            .with_archetype_auto_row(
                [(timeline, tick)],
                &TextLog::new(format!("Log at tick {tick}")),
            )
            .build()
            .expect("failed to build chunk")
    })
}
