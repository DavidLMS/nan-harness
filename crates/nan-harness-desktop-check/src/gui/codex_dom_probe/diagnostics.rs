//! Closed, non-authoritative diagnostics from the retained Codex controller.

use super::RetryClickPhase;
use serde::{Deserialize, Deserializer, Serialize};

// Derived unit-enum decoding also accepts {"variant":null}. This protocol
// requires literal strings and never includes an invalid value in its error.
macro_rules! closed_string_enum {
    (enum $name:ident { $($variant:ident => $wire:literal),+ $(,)? }) => {
        #[derive(Serialize)]
        enum $name {
            $(#[serde(rename = $wire)] $variant),+
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                let value = String::deserialize(deserializer).map_err(|_| {
                    <D::Error as serde::de::Error>::custom("invalid closed Codex diagnostic string")
                })?;
                match value.as_str() {
                    $($wire => Ok(Self::$variant)),+,
                    _ => Err(<D::Error as serde::de::Error>::custom("invalid closed Codex diagnostic string")),
                }
            }
        }
    };
}

// Nullable keys are still required on the wire. Only explicitly optional
// extensions below may disappear when an older controller emits no evidence.
fn required_nullable<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::deserialize(deserializer)
}

pub(super) fn present<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(deserializer).map(Some)
}

closed_string_enum! {
    enum RefusalReason {
        GuardRejected => "guard-rejected",
        Deadline => "deadline",
        ControlCount => "control-count",
        ControlDisabled => "control-disabled",
        MissingHandle => "missing-handle",
        ControlReplaced => "control-replaced",
        SampleBlocked => "sample-blocked",
        FirstHitMissing => "first-hit-missing",
        SecondHitMissing => "second-hit-missing",
        StationaryHitChanged => "stationary-hit-changed",
        FinalSampleChanged => "final-sample-changed",
        FinalPointLost => "final-point-lost",
    }
}

closed_string_enum! {
    enum SampleBlocker {
        UnsupportedControl => "unsupported-control",
        DetachedOrInert => "detached-or-inert",
        Disabled => "disabled",
        Hidden => "hidden",
        ForeignOverlay => "foreign-overlay",
        PointerDisabled => "pointer-disabled",
        AncestorLimit => "ancestor-limit",
    }
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct HitSample {
    #[serde(deserialize_with = "required_nullable")]
    blocked: Option<SampleBlocker>,
    #[serde(deserialize_with = "required_nullable")]
    owned_hit_count: Option<u8>,
    #[serde(deserialize_with = "required_nullable")]
    tested_viewport_count: Option<u8>,
}

impl HitSample {
    fn valid(&self) -> bool {
        match (
            &self.blocked,
            self.owned_hit_count,
            self.tested_viewport_count,
        ) {
            (Some(_), None, None) => true,
            (None, Some(owned), Some(tested)) => owned <= tested && tested <= 9,
            _ => false,
        }
    }

    fn missing_hit(&self) -> bool {
        self.blocked.is_none() && self.owned_hit_count == Some(0)
    }
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct RetryClickRefusal {
    reason: RefusalReason,
    remaining_ms: u32,
    #[serde(deserialize_with = "required_nullable")]
    first: Option<HitSample>,
    #[serde(deserialize_with = "required_nullable")]
    second: Option<HitSample>,
    #[serde(rename = "final", deserialize_with = "required_nullable")]
    final_sample: Option<HitSample>,
}

impl RetryClickRefusal {
    pub(super) fn guard_rejected(&self) -> bool {
        matches!(self.reason, RefusalReason::GuardRejected)
    }

    pub(super) fn valid(&self, phase: Option<RetryClickPhase>) -> bool {
        use RefusalReason as Reason;
        use RetryClickPhase as Phase;
        let samples = [&self.first, &self.second, &self.final_sample];
        if self.remaining_ms > 600_000
            || matches!(self.reason, Reason::Deadline) && self.remaining_ms != 0
            || !samples.into_iter().flatten().all(HitSample::valid)
        {
            return false;
        }
        let phase_valid = match self.reason {
            Reason::GuardRejected | Reason::Deadline => matches!(
                phase,
                Some(
                    Phase::Admission
                        | Phase::Revalidate
                        | Phase::SampleSecond
                        | Phase::SampleFinal
                        | Phase::PostGuard
                )
            ),
            Reason::ControlCount | Reason::ControlDisabled => {
                matches!(phase, Some(Phase::Admission | Phase::Revalidate))
            }
            Reason::MissingHandle => matches!(phase, Some(Phase::Capture)),
            Reason::ControlReplaced => matches!(phase, Some(Phase::Revalidate)),
            Reason::SampleBlocked => {
                matches!(phase, Some(Phase::SampleFirst | Phase::SampleSecond))
            }
            Reason::FirstHitMissing | Reason::SecondHitMissing | Reason::StationaryHitChanged => {
                matches!(phase, Some(Phase::SampleSecond))
            }
            Reason::FinalSampleChanged | Reason::FinalPointLost => {
                matches!(phase, Some(Phase::SampleFinal))
            }
        };
        phase_valid
            && match self.reason {
                Reason::SampleBlocked => [&self.first, &self.second]
                    .into_iter()
                    .flatten()
                    .any(|sample| sample.blocked.is_some()),
                Reason::FirstHitMissing => self.first.as_ref().is_some_and(HitSample::missing_hit),
                Reason::SecondHitMissing => {
                    self.second.as_ref().is_some_and(HitSample::missing_hit)
                }
                _ => true,
            }
    }
}

closed_string_enum! {
    enum SourceVersion {
        Frozen => "26.1007.21434",
    }
}

closed_string_enum! {
    enum RendererHash {
        Frozen => "2b78662247051c8b0905e1afbe0839187b8267e9181e179bc422322e4010433b",
    }
}

closed_string_enum! {
    enum SpotlightStatus {
        Verified => "verified",
        Rejected => "rejected",
    }
}

closed_string_enum! {
    enum SpotlightFailure {
        MainFocus => "main-focus",
        AuxiliaryFocus => "auxiliary-focus",
        InitialScope => "initial-scope",
        ScopeChanged => "scope-changed",
        SourceOpen => "source-open",
        SourceTransition => "source-transition",
        ForeignControls => "foreign-controls",
        Identity => "identity",
        Ownership => "ownership",
        Deadline => "deadline",
        Query => "query",
    }
}

closed_string_enum! {
    enum RejectionCheck {
        BodyAnchor => "body-anchor",
        BodyStyle => "body-style",
        InitialControls => "initial-controls",
        ForbiddenNodes => "forbidden-nodes",
        ControlLocation => "control-location",
        ControlPointer => "control-pointer",
        EditableScope => "editable-scope",
        MainControls => "main-controls",
        ToastAnnouncer => "toast-announcer",
    }
}

closed_string_enum! {
    enum ControlKind {
        Command => "command",
        Input => "input",
        List => "list",
        DisabledButton => "disabled-button",
        Button => "button",
        Anchor => "anchor",
        Select => "select",
        Other => "other",
        RadixFocusGuard => "radix-focus-guard",
    }
}

// A missing legacy key, an unavailable null and an observed count are distinct.
#[derive(Deserialize, Serialize)]
#[serde(untagged)]
enum ToastAnnouncerCount {
    Count(u8),
    Unavailable,
}

impl ToastAnnouncerCount {
    fn valid(&self) -> bool {
        match self {
            Self::Count(count) => *count <= 1,
            Self::Unavailable => true,
        }
    }

    fn verified(&self) -> bool {
        matches!(self, Self::Count(1))
    }
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct SpotlightObservation {
    source_version: SourceVersion,
    renderer_sha256: RendererHash,
    status: SpotlightStatus,
    #[serde(deserialize_with = "required_nullable")]
    failure: Option<SpotlightFailure>,
    stable_samples: u8,
    #[serde(deserialize_with = "required_nullable")]
    input_count: Option<u16>,
    #[serde(deserialize_with = "required_nullable")]
    list_count: Option<u16>,
    #[serde(deserialize_with = "required_nullable")]
    editable_count: Option<u16>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    rejection_check: Option<RejectionCheck>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    control_kind: Option<ControlKind>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    toast_announcer_count: Option<ToastAnnouncerCount>,
}

impl SpotlightObservation {
    pub(super) fn valid(&self) -> bool {
        let counts = [self.input_count, self.list_count, self.editable_count];
        if self.stable_samples > 2
            || counts.into_iter().flatten().any(|count| count > 4096)
            || self
                .toast_announcer_count
                .as_ref()
                .is_some_and(|count| !count.valid())
        {
            return false;
        }
        let state_valid = match self.status {
            SpotlightStatus::Verified => {
                self.failure.is_none()
                    && self.stable_samples == 2
                    && counts.into_iter().all(|count| count == Some(1))
                    && self
                        .toast_announcer_count
                        .as_ref()
                        .is_none_or(ToastAnnouncerCount::verified)
            }
            SpotlightStatus::Rejected => self.failure.is_some(),
        };
        state_valid
            && (self.rejection_check.is_none()
                || matches!(self.status, SpotlightStatus::Rejected)
                    && matches!(self.failure, Some(SpotlightFailure::ForeignControls)))
            && (self.control_kind.is_none()
                || matches!(
                    self.rejection_check,
                    Some(RejectionCheck::ControlLocation | RejectionCheck::ControlPointer)
                ))
    }
}
