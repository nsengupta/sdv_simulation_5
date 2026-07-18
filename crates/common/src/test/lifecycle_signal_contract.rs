//! Contract tests for the strict CAN `0x100` lifecycle codec.

use crate::signals::{ID_LIFECYCLE, LifecycleCommand};
use socketcan::{CanFrame, EmbeddedFrame, ExtendedId, StandardId};

fn standard_frame(id: u16, data: &[u8]) -> CanFrame {
    CanFrame::new(StandardId::new(id).expect("standard CAN id"), data).expect("CAN frame")
}

#[test]
fn lifecycle_power_on_encodes_to_exact_can_contract() {
    let frame = LifecycleCommand::PowerOn
        .to_can_frame()
        .expect("encode PowerOn");

    assert_eq!(frame.id(), StandardId::new(ID_LIFECYCLE).unwrap().into());
    assert_eq!(frame.data(), &[1, 0, 0, 0, 0, 0, 0, 0]);
}

#[test]
fn lifecycle_power_off_encodes_to_exact_can_contract() {
    let frame = LifecycleCommand::PowerOff
        .to_can_frame()
        .expect("encode PowerOff");

    assert_eq!(frame.id(), StandardId::new(ID_LIFECYCLE).unwrap().into());
    assert_eq!(frame.data(), &[0, 0, 0, 0, 0, 0, 0, 0]);
}

#[test]
fn lifecycle_commands_round_trip() {
    for command in [LifecycleCommand::PowerOn, LifecycleCommand::PowerOff] {
        let frame = command.to_can_frame().expect("encode lifecycle");
        assert_eq!(LifecycleCommand::from_can_frame(&frame), Some(command));
    }
}

#[test]
fn lifecycle_decoder_rejects_wrong_standard_id() {
    let frame = standard_frame(ID_LIFECYCLE + 1, &[1, 0, 0, 0, 0, 0, 0, 0]);
    assert_eq!(LifecycleCommand::from_can_frame(&frame), None);
}

#[test]
fn lifecycle_decoder_rejects_extended_id() {
    let frame = CanFrame::new(
        ExtendedId::new(ID_LIFECYCLE as u32).expect("extended CAN id"),
        &[1, 0, 0, 0, 0, 0, 0, 0],
    )
    .expect("CAN frame");

    assert_eq!(LifecycleCommand::from_can_frame(&frame), None);
}

#[test]
fn lifecycle_decoder_rejects_non_eight_byte_payload() {
    let frame = standard_frame(ID_LIFECYCLE, &[1, 0, 0, 0, 0, 0, 0]);
    assert_eq!(LifecycleCommand::from_can_frame(&frame), None);
}

#[test]
fn lifecycle_decoder_rejects_unknown_opcode() {
    let frame = standard_frame(ID_LIFECYCLE, &[2, 0, 0, 0, 0, 0, 0, 0]);
    assert_eq!(LifecycleCommand::from_can_frame(&frame), None);
}

#[test]
fn lifecycle_decoder_rejects_nonzero_reserved_byte() {
    let frame = standard_frame(ID_LIFECYCLE, &[1, 0, 0, 1, 0, 0, 0, 0]);
    assert_eq!(LifecycleCommand::from_can_frame(&frame), None);
}
