//! Pure presentation helpers for vehicle metrics (no I/O).

use super::constants::SPEED_EXTREME_OPERATION_THRESHOLD_KPH;

/// Build a filled bar of exactly `bar_width` characters for `speed_kph`.
///
/// Full scale is [`SPEED_EXTREME_OPERATION_THRESHOLD_KPH`]. Speeds at or above that clamp to a
/// full bar. An empty `bar_width` yields an empty string.
pub fn format_speed_bar(speed_kph: u16, bar_width: usize) -> String {
    if bar_width == 0 {
        return String::new();
    }
    let full = usize::from(SPEED_EXTREME_OPERATION_THRESHOLD_KPH).max(1);
    let filled = (usize::from(speed_kph).min(full) * bar_width) / full;
    let mut out = String::with_capacity(bar_width);
    for i in 0..bar_width {
        out.push(if i < filled { '|' } else { '.' });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vehicle_physics::SPEED_EXTREME_OPERATION_THRESHOLD_KPH;

    #[test]
    fn zero_speed_is_all_empty() {
        assert_eq!(format_speed_bar(0, 10), "..........");
    }

    #[test]
    fn full_scale_fills_bar() {
        assert_eq!(
            format_speed_bar(SPEED_EXTREME_OPERATION_THRESHOLD_KPH, 10),
            "||||||||||"
        );
    }

    #[test]
    fn above_full_scale_clamps() {
        assert_eq!(
            format_speed_bar(SPEED_EXTREME_OPERATION_THRESHOLD_KPH + 40, 8),
            "||||||||"
        );
    }

    #[test]
    fn mid_speed_fills_proportionally() {
        let bar = format_speed_bar(80, 10); // half of 160
        assert_eq!(bar.chars().filter(|c| *c == '|').count(), 5);
        assert_eq!(bar.len(), 10);
    }

    #[test]
    fn zero_width_is_empty() {
        assert_eq!(format_speed_bar(100, 0), "");
    }
}
