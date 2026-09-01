use super::ExecutionOutcome;
use std::mem::size_of;

pub(super) const STARTUP_MESSAGES: &[&str] = &[
    "Save the tokens, save the world.",
    "With great token power comes great responsibility.",
    "Build something legen—wait for it—dary!",
    "Enjoy building with NaN. Please don’t accidentally cook a GPU.",
    "Burn tokens responsibly. Someone has to keep the GPUs cool.",
    "The cluster is shared. The weird ideas are all yours.",
    "May your prompts be sharp and your context window roomy.",
    "One does not simply waste a perfectly good context window.",
    "Do, or do not build. There is no try.",
    "The build is strong with this one.",
    "Live long and prosper. Keep the tests green.",
    "The tokens are calling. Answer with a pull request.",
    "A small prompt for you, a giant leap for your codebase.",
    "Think openly. Build boldly. Leave the cluster some oxygen.",
    "The first rule of Token Club: spend them on something worth shipping.",
    "Today’s forecast: 100% chance of shipping something weird.",
    "Open models, shared GPUs, questionable ideas. Let’s build.",
    "You have entered the danger zone. Keep an eye on those tokens.",
    "The answer is 42. The question is: what are you building?",
    "May the source be with you.",
];

pub(super) const SUCCESS_MESSAGES: &[&str] = &[
    "The GPUs are taking a well-deserved break.",
    "You did it. The datacenter can stop sweating now.",
    "Session complete. The cluster lives to build another day.",
    "That’s a wrap. Go hydrate; the GPUs already are.",
    "Tokens spent, code advanced, planet mildly relieved.",
    "The agent has left the building. The code remains.",
    "Mission accomplished. Please remember where you saved the changes.",
    "The build is over. Time for the silicon to cool down.",
    "You came. You prompted. You shipped. Probably.",
    "Session closed. High five to the shared cluster.",
    "The context window is closed. The possibilities are not.",
    "The datacenter is no longer on fire. Nice work.",
    "That’s all, folks. Keep the weird ideas coming.",
    "The tokens have returned to the wild. Use the next batch wisely.",
    "Your session has ended. Your next side project is already judging you.",
    "The cluster says thanks. The tests say you’re welcome.",
    "No tokens were harmed in the making of this session.",
    "Work complete. Time to let the silicon breathe.",
    "The final prompt has landed. See you in the next build.",
    "NaN out. Keep building without limits.",
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

fn random_message(messages: &[&'static str]) -> &'static str {
    let mut bytes = [0; size_of::<usize>()];
    if getrandom::fill(&mut bytes).is_err() {
        return messages[0];
    }
    choose_message(messages, usize::from_ne_bytes(bytes))
}

fn choose_message(messages: &[&'static str], random_value: usize) -> &'static str {
    messages[random_value % messages.len()]
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
        assert_eq!(choose_message(STARTUP_MESSAGES, 0), STARTUP_MESSAGES[0]);
        assert_eq!(choose_message(STARTUP_MESSAGES, 19), STARTUP_MESSAGES[19]);
        assert_eq!(choose_message(STARTUP_MESSAGES, 20), STARTUP_MESSAGES[0]);
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
