use super::projection::{Projector, ProjectionError};
use crate::digital_twin::TwinMessage;
use crate::domain_types::TwinIngressEvent;
use crate::fsm::{FrontHeadlampIncompleteCause, FrontHeadlampSwitchDirection, FsmEvent};
use crate::signals::{LifecycleCommand, VssSignal};

#[derive(Debug, Default, Clone, Copy)]
pub struct IngressToFsmProjector;

impl Projector<TwinIngressEvent, TwinMessage> for IngressToFsmProjector {
    fn project(&self, input: TwinIngressEvent) -> Result<TwinMessage, ProjectionError> {
        let fsm = match input {
            TwinIngressEvent::Lifecycle(LifecycleCommand::PowerOn) => FsmEvent::PowerOn,
            TwinIngressEvent::Lifecycle(LifecycleCommand::PowerOff) => FsmEvent::PowerOff,
            TwinIngressEvent::Telemetry(vss) => match vss {
                VssSignal::Speed(_) => {
                    return Err(ProjectionError::InvalidPayload(
                        "observed Speed not wired yet; twin derives speed from EngineRpm",
                    ));
                }
                VssSignal::EngineRpm(rpm) => FsmEvent::UpdateRpm(rpm),
                VssSignal::AmbientLux(lux) => FsmEvent::UpdateAmbientLux(lux),
                VssSignal::RainDetected(true)  => FsmEvent::RainsStarted,
                VssSignal::RainDetected(false) => FsmEvent::RainsStopped,
            },
            TwinIngressEvent::TimerTick => FsmEvent::TimerTick,
            TwinIngressEvent::SystemReset => FsmEvent::PowerOff,
            TwinIngressEvent::FrontHeadlampCommandConfirmed { on_command } => {
                if on_command {
                    FsmEvent::FrontHeadlampOnAck
                } else {
                    FsmEvent::FrontHeadlampOffAck
                }
            }
            TwinIngressEvent::FrontHeadlampCommandRejected { on_command } => {
                FsmEvent::FrontHeadlampActuationIncomplete {
                    direction: if on_command {
                        FrontHeadlampSwitchDirection::On
                    } else {
                        FrontHeadlampSwitchDirection::Off
                    },
                    cause: FrontHeadlampIncompleteCause::NegativeAck,
                }
            }
        };
        Ok(TwinMessage::Fsm(fsm))
    }
}
