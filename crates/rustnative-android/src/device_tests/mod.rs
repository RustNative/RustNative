//! The device suite: tests that need a real activity, run on the device
//! through the host library's instrumentation
//! (`am instrument -w <package>/dev.rustnative.android.RnInstrumentation`;
//! `tools/android-device-test.sh`).
//!
//! Each test is an ordinary function run on the instrumentation thread;
//! its body runs on the main thread through [`harness::on_main`], and input
//! it injects through the instrumentation arrives the way a person's
//! does. Each reports its start and outcome as `am instrument -r` prints
//! them.

// A test states its expectations by panicking, as `#[test]`s do.
#![allow(clippy::expect_used, clippy::unwrap_used, reason = "tests assert by panicking")]

mod accessibility;
mod conformance;
mod graphics;
pub(crate) mod harness;
mod input;
mod realization;
mod services;
mod styling;
mod surfaces;

pub(crate) use harness::activity_ready;

use crate::jni_host::{Arg, JavaRef, call};
use harness::Instrumentation;

type Test = fn(&Instrumentation);

/// Every device test, by name.
const TESTS: &[(&str, Test)] = &[
    (
        "realization::every_kind_is_its_platform_view_at_its_rectangle",
        realization::every_kind_is_its_platform_view_at_its_rectangle,
    ),
    ("realization::a_tap_reaches_update", realization::a_tap_reaches_update),
    (
        "realization::typing_reports_each_change_and_the_framework_never_echoes",
        realization::typing_reports_each_change_and_the_framework_never_echoes,
    ),
    (
        "realization::controls_report_and_stay_controlled",
        realization::controls_report_and_stay_controlled,
    ),
    ("realization::scrolling_renders_nothing", realization::scrolling_renders_nothing),
    (
        "realization::a_task_on_another_thread_wakes_the_main_looper",
        realization::a_task_on_another_thread_wakes_the_main_looper,
    ),
    (
        "realization::labels_measure_with_their_text_and_wrap",
        realization::labels_measure_with_their_text_and_wrap,
    ),
    (
        "realization::churn_returns_the_census_to_its_baseline",
        realization::churn_returns_the_census_to_its_baseline,
    ),
    (
        "realization::the_shared_guarantee_suites_hold_on_android",
        realization::the_shared_guarantee_suites_hold_on_android,
    ),
    (
        "styling::the_android_table_is_what_the_backend_applies",
        styling::the_android_table_is_what_the_backend_applies,
    ),
    (
        "styling::night_mode_reaches_the_environment_and_keeps_the_views",
        styling::night_mode_reaches_the_environment_and_keeps_the_views,
    ),
    (
        "styling::font_scale_reaches_the_environment_and_remeasures",
        styling::font_scale_reaches_the_environment_and_remeasures,
    ),
    (
        "styling::right_to_left_mirrors_with_the_same_views",
        styling::right_to_left_mirrors_with_the_same_views,
    ),
    ("styling::the_safe_area_is_the_windows_insets", styling::the_safe_area_is_the_windows_insets),
    (
        "accessibility::the_tree_carries_the_portable_model",
        accessibility::the_tree_carries_the_portable_model,
    ),
    (
        "accessibility::a_screen_readers_actions_reach_the_component",
        accessibility::a_screen_readers_actions_reach_the_component,
    ),
    (
        "accessibility::the_tree_is_served_to_a_running_talkback",
        accessibility::the_tree_is_served_to_a_running_talkback,
    ),
    (
        "graphics::a_transition_runs_across_frames_and_settles",
        graphics::a_transition_runs_across_frames_and_settles,
    ),
    (
        "graphics::a_hundred_thousand_rows_realize_a_screenful_and_recycle",
        graphics::a_hundred_thousand_rows_realize_a_screenful_and_recycle,
    ),
    ("graphics::a_canvas_draws_its_list", graphics::a_canvas_draws_its_list),
    (
        "graphics::a_native_surface_is_a_live_window_the_size_of_its_node",
        graphics::a_native_surface_is_a_live_window_the_size_of_its_node,
    ),
    ("graphics::web_content_loads_in_a_web_view", graphics::web_content_loads_in_a_web_view),
    ("input::a_key_reaches_the_application", input::a_key_reaches_the_application),
    (
        "input::system_back_is_the_back_command_while_it_is_enabled",
        input::system_back_is_the_back_command_while_it_is_enabled,
    ),
    (
        "input::a_controller_button_reaches_the_node_that_wants_it",
        input::a_controller_button_reaches_the_node_that_wants_it,
    ),
    (
        "input::an_input_method_composes_into_a_custom_text_target",
        input::an_input_method_composes_into_a_custom_text_target,
    ),
    ("services::the_clipboard_round_trips", services::the_clipboard_round_trips),
    ("services::a_url_nothing_opens_says_so", services::a_url_nothing_opens_says_so),
    (
        "services::a_notification_posts_on_its_channel_with_its_actions",
        services::a_notification_posts_on_its_channel_with_its_actions,
    ),
    (
        "services::permission_states_follow_the_system",
        services::permission_states_follow_the_system,
    ),
    (
        "services::secure_storage_round_trips_in_the_keystore",
        services::secure_storage_round_trips_in_the_keystore,
    ),
    (
        "services::http_honours_the_platform_policy_and_pins",
        services::http_honours_the_platform_policy_and_pins,
    ),
    ("services::icu_formats_collates_and_cases", services::icu_formats_collates_and_cases),
    ("services::images_decode_and_downscale", services::images_decode_and_downscale),
    ("services::a_print_job_becomes_a_pdf", services::a_print_job_becomes_a_pdf),
    (
        "services::state_persists_in_the_application_files",
        services::state_persists_in_the_application_files,
    ),
    (
        "services::push_and_billing_say_why_they_are_unavailable",
        services::push_and_billing_say_why_they_are_unavailable,
    ),
    (
        "services::a_dismissed_document_picker_answers_none",
        services::a_dismissed_document_picker_answers_none,
    ),
    (
        "surfaces::a_widget_keeps_what_the_application_sent",
        surfaces::a_widget_keeps_what_the_application_sent,
    ),
    ("surfaces::a_tile_keeps_its_state", surfaces::a_tile_keeps_its_state),
    (
        "surfaces::an_ongoing_activity_shows_until_it_ends",
        surfaces::an_ongoing_activity_shows_until_it_ends,
    ),
    (
        "surfaces::the_jump_list_becomes_launcher_shortcuts",
        surfaces::the_jump_list_becomes_launcher_shortcuts,
    ),
    (
        "surfaces::a_constrained_job_runs_through_job_scheduler",
        surfaces::a_constrained_job_runs_through_job_scheduler,
    ),
    (
        "conformance::a_node_s_cursor_is_its_view_s_pointer_icon",
        conformance::a_node_s_cursor_is_its_view_s_pointer_icon,
    ),
    (
        "conformance::mappers_extend_and_replace_what_the_backend_applies",
        conformance::mappers_extend_and_replace_what_the_backend_applies,
    ),
    (
        "conformance::a_panicking_handler_ends_the_application_not_the_process",
        conformance::a_panicking_handler_ends_the_application_not_the_process,
    ),
];

fn report(instrumentation: &JavaRef, name: &str, status: i32, message: Option<&str>) {
    let _ = call(
        instrumentation,
        "report",
        "(Ljava/lang/String;ILjava/lang/String;)V",
        &[Arg::Str(name), Arg::Int(status), Arg::OptStr(message)],
    );
}

/// Runs every test whose name contains `filter` (instrumentation thread).
pub(crate) fn run(instrumentation: &JavaRef, filter: &str) {
    let driver = Instrumentation(instrumentation.clone());
    for (name, test) in TESTS {
        if !name.contains(filter) {
            continue;
        }
        report(instrumentation, name, 1, None);
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| test(&driver)));
        match outcome {
            Ok(()) => report(instrumentation, name, 0, None),
            Err(payload) => {
                let message = crate::backend::panic_message(payload.as_ref());
                crate::log::error(&format!("device test {name} failed: {message}"));
                report(instrumentation, name, -2, Some(&message));
            }
        }
        // Whatever a test left running goes before the next starts.
        harness::on_main(harness::stop_running);
        driver.wait_idle();
    }
}
