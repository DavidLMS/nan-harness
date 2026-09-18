use super::ExecutionOutcome;
use std::mem::size_of;

pub(super) const STARTUP_MESSAGES: &[fn(nan_harness_i18n::Locale) -> &'static str] = &[
    nan_harness_i18n::messages::personality_save_the_tokens_save_the_world_text,
    nan_harness_i18n::messages::personality_with_great_token_power_comes_great_responsibility_text,
    nan_harness_i18n::messages::personality_build_something_legen_wait_for_it_dary_text,
    nan_harness_i18n::messages::personality_enjoy_building_with_nan_please_don_t_accidentally_cook_a_gpu_text,
    nan_harness_i18n::messages::personality_burn_tokens_responsibly_someone_has_to_keep_the_gpus_cool_text,
    nan_harness_i18n::messages::personality_the_cluster_is_shared_the_weird_ideas_are_all_yours_text,
    nan_harness_i18n::messages::personality_may_your_prompts_be_sharp_and_your_context_window_roomy_text,
    nan_harness_i18n::messages::personality_one_does_not_simply_waste_a_perfectly_good_context_window_text,
    nan_harness_i18n::messages::personality_do_or_do_not_build_there_is_no_try_text,
    nan_harness_i18n::messages::personality_the_build_is_strong_with_this_one_text,
    nan_harness_i18n::messages::personality_live_long_and_prosper_keep_the_tests_green_text,
    nan_harness_i18n::messages::personality_the_tokens_are_calling_answer_with_a_pull_request_text,
    nan_harness_i18n::messages::personality_a_small_prompt_for_you_a_giant_leap_for_your_codebase_text,
    nan_harness_i18n::messages::personality_think_openly_build_boldly_leave_the_cluster_some_oxygen_text,
    nan_harness_i18n::messages::personality_the_first_rule_of_token_club_spend_them_on_something_worth_shipping_text,
    nan_harness_i18n::messages::personality_today_s_forecast_100_chance_of_shipping_something_weird_text,
    nan_harness_i18n::messages::personality_open_models_shared_gpus_questionable_ideas_let_s_build_text,
    nan_harness_i18n::messages::personality_you_have_entered_the_danger_zone_keep_an_eye_on_those_tokens_text,
    nan_harness_i18n::messages::personality_the_answer_is_42_the_question_is_what_are_you_building_text,
    nan_harness_i18n::messages::personality_may_the_source_be_with_you_text,
];

pub(super) const SUCCESS_MESSAGES: &[fn(nan_harness_i18n::Locale) -> &'static str] = &[
    nan_harness_i18n::messages::personality_the_gpus_are_taking_a_well_deserved_break_text,
    nan_harness_i18n::messages::personality_you_did_it_the_datacenter_can_stop_sweating_now_text,
    nan_harness_i18n::messages::personality_session_complete_the_cluster_lives_to_build_another_day_text,
    nan_harness_i18n::messages::personality_that_s_a_wrap_go_hydrate_the_gpus_already_are_text,
    nan_harness_i18n::messages::personality_tokens_spent_code_advanced_planet_mildly_relieved_text,
    nan_harness_i18n::messages::personality_the_agent_has_left_the_building_the_code_remains_text,
    nan_harness_i18n::messages::personality_mission_accomplished_please_remember_where_you_saved_the_changes_text,
    nan_harness_i18n::messages::personality_the_build_is_over_time_for_the_silicon_to_cool_down_text,
    nan_harness_i18n::messages::personality_you_came_you_prompted_you_shipped_probably_text,
    nan_harness_i18n::messages::personality_session_closed_high_five_to_the_shared_cluster_text,
    nan_harness_i18n::messages::personality_the_context_window_is_closed_the_possibilities_are_not_text,
    nan_harness_i18n::messages::personality_the_datacenter_is_no_longer_on_fire_nice_work_text,
    nan_harness_i18n::messages::personality_that_s_all_folks_keep_the_weird_ideas_coming_text,
    nan_harness_i18n::messages::personality_the_tokens_have_returned_to_the_wild_use_the_next_batch_wisely_text,
    nan_harness_i18n::messages::personality_your_session_has_ended_your_next_side_project_is_already_judging_you_text,
    nan_harness_i18n::messages::personality_the_cluster_says_thanks_the_tests_say_you_re_welcome_text,
    nan_harness_i18n::messages::personality_no_tokens_were_harmed_in_the_making_of_this_session_text,
    nan_harness_i18n::messages::personality_work_complete_time_to_let_the_silicon_breathe_text,
    nan_harness_i18n::messages::personality_the_final_prompt_has_landed_see_you_in_the_next_build_text,
    nan_harness_i18n::messages::personality_nan_out_keep_building_without_limits_text,
];

pub(super) fn random_startup_message(interactive: bool) -> Option<&'static str> {
    interactive.then(|| random_message(STARTUP_MESSAGES))
}

pub(super) fn random_success_message(
    interactive: bool,
    outcome: ExecutionOutcome,
) -> Option<&'static str> {
    (interactive && matches!(outcome, ExecutionOutcome::Succeeded))
        .then(|| random_message(SUCCESS_MESSAGES))
}

fn random_message(messages: &[fn(nan_harness_i18n::Locale) -> &'static str]) -> &'static str {
    let mut bytes = [0; size_of::<usize>()];
    if getrandom::fill(&mut bytes).is_err() {
        return messages[0](nan_harness_i18n::locale());
    }
    choose_message(messages, usize::from_ne_bytes(bytes))
}

fn choose_message(
    messages: &[fn(nan_harness_i18n::Locale) -> &'static str],
    random_value: usize,
) -> &'static str {
    messages[random_value % messages.len()](nan_harness_i18n::locale())
}

#[cfg(test)]
mod tests {
    use super::{
        STARTUP_MESSAGES, SUCCESS_MESSAGES, choose_message, random_startup_message,
        random_success_message,
    };
    use nan_harness_runtime::{ExecutionOutcome, SignalKind};

    #[test]
    fn collections_have_twenty_messages_each() {
        assert_eq!(STARTUP_MESSAGES.len(), 20);
        assert_eq!(SUCCESS_MESSAGES.len(), 20);
    }

    #[test]
    fn deterministic_selection_wraps_at_collection_length() {
        assert_eq!(
            choose_message(STARTUP_MESSAGES, 0),
            STARTUP_MESSAGES[0](nan_harness_i18n::Locale::En)
        );
        assert_eq!(
            choose_message(STARTUP_MESSAGES, 19),
            STARTUP_MESSAGES[19](nan_harness_i18n::Locale::En)
        );
        assert_eq!(
            choose_message(STARTUP_MESSAGES, 20),
            STARTUP_MESSAGES[0](nan_harness_i18n::Locale::En)
        );
    }

    #[test]
    fn personality_messages_require_an_interactive_run() {
        assert!(random_startup_message(false).is_none());
        assert!(random_success_message(false, ExecutionOutcome::Succeeded).is_none());
        assert!(random_startup_message(true).is_some());
    }

    #[test]
    fn success_messages_are_only_returned_for_successful_runs() {
        assert!(random_success_message(true, ExecutionOutcome::Succeeded).is_some());
        assert!(random_success_message(true, ExecutionOutcome::Failed).is_none());
        assert!(
            random_success_message(true, ExecutionOutcome::Cancelled(SignalKind::Interrupt))
                .is_none()
        );
    }
}
