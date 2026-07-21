//! L0 foundation: vehicle calibration constants and pure kinematic formulas.
//!
//! Depends on std only. No FSM, runtime, or I/O types.
//!
//! **Consumers (one constitution):** L2 [`transition_map`], L3 state laws, L1 zone handlers
//! (e.g. headlamp lux bands), and **operational detectors**. New detectors must reuse predicates and thresholds
//! from this module rather than duplicating physics locally.

pub mod constants;
pub mod display;
pub mod kinematics;

pub use constants::{
    EXTREME_OPERATION_WARNING_MESSAGE, FRONT_HEADLAMP_OFF_ACK_WAIT, FRONT_HEADLAMP_ON_ACK_WAIT,
    LUX_OFF_THRESHOLD, LUX_ON_THRESHOLD, RPM_DRIVING_THRESHOLD, RPM_EXTREME_OPERATION_THRESHOLD,
    RPM_IDLE, RPM_REDLINE_THRESHOLD, RPM_STRESS_DURATION_THRESHOLD_SECS, SPEED_BAND_GREEN_MAX_KPH,
    SPEED_BAND_YELLOW_MAX_KPH, SPEED_EXTREME_OPERATION_THRESHOLD_KPH,
    SPEED_THRESHOLD_WARNING_MESSAGE, extreme_operation_active, operational_warning_active,
    speed_threshold_exceeded,
};
pub use display::{SpeedBand, SpeedBarCell, format_speed_bar, speed_band, speed_bar_cells};
pub use kinematics::calculate_speed_from_rpm;
