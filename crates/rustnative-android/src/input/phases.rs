//! The host library's back phases in the portable model.

use rustnative_core::{BackEdge, BackPhase, Scalar};

use crate::protocol;

/// The portable phase for the host library's (`protocol::BACK_*`), or
/// `None` for the completed gesture (which is the command itself).
pub(crate) fn portable_phase(phase: i32, progress: f32, edge: i32) -> Option<BackPhase> {
    Some(match phase {
        protocol::BACK_STARTED => BackPhase::Started {
            edge: match edge {
                1 => BackEdge::Left,
                2 => BackEdge::Right,
                _ => BackEdge::None,
            },
        },
        protocol::BACK_PROGRESSED => {
            BackPhase::Progressed { progress: Scalar::new(progress.clamp(0.0, 1.0)) }
        }
        protocol::BACK_CANCELLED => BackPhase::Cancelled,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn back_phases_map() {
        assert_eq!(
            portable_phase(protocol::BACK_STARTED, 0.0, 2),
            Some(BackPhase::Started { edge: BackEdge::Right })
        );
        assert_eq!(
            portable_phase(protocol::BACK_PROGRESSED, 1.5, 0),
            Some(BackPhase::Progressed { progress: Scalar::ONE })
        );
        assert_eq!(portable_phase(protocol::BACK_CANCELLED, 0.0, 0), Some(BackPhase::Cancelled));
        assert_eq!(portable_phase(protocol::BACK_INVOKED, 1.0, 0), None);
    }
}
