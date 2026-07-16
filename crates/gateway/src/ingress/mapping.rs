use common::facade::{LifecycleCommand, TwinIngressEvent, VssSignal};
use socketcan::CanFrame;

/// Decode a generic CAN frame into the canonical, transport-independent twin ingress vocabulary.
///
/// Device-specific actuator response frames are intentionally handled by their correlation-aware
/// policies after this generic lifecycle/telemetry decoder returns `None`.
pub fn can_frame_to_twin_ingress(frame: &CanFrame) -> Option<TwinIngressEvent> {
    if let Some(command) = LifecycleCommand::from_can_frame(frame) {
        return Some(TwinIngressEvent::Lifecycle(command));
    }

    VssSignal::from_can_frame(frame).map(TwinIngressEvent::Telemetry)
}

#[cfg(test)]
mod tests {
    use super::can_frame_to_twin_ingress;
    use common::facade::{LifecycleCommand, TwinIngressEvent, VssSignal};

    #[test]
    fn lifecycle_power_on_frame_maps_to_twin_ingress() {
        let frame = LifecycleCommand::PowerOn
            .to_can_frame()
            .expect("encode PowerOn");

        assert!(matches!(
            can_frame_to_twin_ingress(&frame),
            Some(TwinIngressEvent::Lifecycle(LifecycleCommand::PowerOn))
        ));
    }

    #[test]
    fn lifecycle_power_off_frame_maps_to_twin_ingress() {
        let frame = LifecycleCommand::PowerOff
            .to_can_frame()
            .expect("encode PowerOff");

        assert!(matches!(
            can_frame_to_twin_ingress(&frame),
            Some(TwinIngressEvent::Lifecycle(LifecycleCommand::PowerOff))
        ));
    }

    #[test]
    fn engine_rpm_frame_maps_to_twin_telemetry() {
        let frame = VssSignal::EngineRpm(4567)
            .to_can_frame()
            .expect("encode RPM");

        assert!(matches!(
            can_frame_to_twin_ingress(&frame),
            Some(TwinIngressEvent::Telemetry(VssSignal::EngineRpm(4567)))
        ));
    }

    #[test]
    fn unknown_frame_is_not_twin_ingress() {
        use socketcan::{CanFrame, EmbeddedFrame, StandardId};

        let frame = CanFrame::new(StandardId::new(0x7ff).unwrap(), &[0; 8]).unwrap();
        assert!(can_frame_to_twin_ingress(&frame).is_none());
    }
}
