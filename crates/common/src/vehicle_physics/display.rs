//! Pure presentation helpers for vehicle metrics (no I/O).

use super::constants::{
    SPEED_BAND_GREEN_MAX_KPH, SPEED_BAND_YELLOW_MAX_KPH, SPEED_EXTREME_OPERATION_THRESHOLD_KPH,
};

/// Speed display band for Dashboard colouring (one truth with [`SPEED_BAND_*`] constants).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpeedBand {
    /// `0..=SPEED_BAND_GREEN_MAX_KPH`
    Green,
    /// `(green_max + 1)..=SPEED_BAND_YELLOW_MAX_KPH`
    Yellow,
    /// Above yellow max (includes speeds at/above full scale)
    Red,
}

/// One cell of a zoned speed bar: fill glyph and scale-zone colour token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpeedBarCell {
    pub filled: bool,
    pub band: SpeedBand,
}

/// Classify ground speed into a display band.
pub fn speed_band(speed_kph: u16) -> SpeedBand {
    if speed_kph <= SPEED_BAND_GREEN_MAX_KPH {
        SpeedBand::Green
    } else if speed_kph <= SPEED_BAND_YELLOW_MAX_KPH {
        SpeedBand::Yellow
    } else {
        SpeedBand::Red
    }
}

/// Build zoned bar cells of exactly `bar_width`.
///
/// Full scale is [`SPEED_EXTREME_OPERATION_THRESHOLD_KPH`]. Speeds at or above that clamp to a
/// full bar. Each cell’s [`SpeedBand`] follows its place on the 0…full scale (zoned segments).
/// Empty cells use `filled == false` (render as `.`).
pub fn speed_bar_cells(speed_kph: u16, bar_width: usize) -> Vec<SpeedBarCell> {
    if bar_width == 0 {
        return Vec::new();
    }
    let full = usize::from(SPEED_EXTREME_OPERATION_THRESHOLD_KPH).max(1);
    let filled = (usize::from(speed_kph).min(full) * bar_width) / full;
    (0..bar_width)
        .map(|i| {
            // Upper edge of this cell on the 0…full scale (km/h).
            let zone_kph = (((i + 1) * full) / bar_width) as u16;
            SpeedBarCell {
                filled: i < filled,
                band: speed_band(zone_kph),
            }
        })
        .collect()
}

/// Build a filled bar of exactly `bar_width` characters for `speed_kph`.
///
/// Full scale is [`SPEED_EXTREME_OPERATION_THRESHOLD_KPH`]. Speeds at or above that clamp to a
/// full bar. An empty `bar_width` yields an empty string.
pub fn format_speed_bar(speed_kph: u16, bar_width: usize) -> String {
    speed_bar_cells(speed_kph, bar_width)
        .into_iter()
        .map(|c| if c.filled { '|' } else { '.' })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vehicle_physics::{
        SPEED_BAND_GREEN_MAX_KPH, SPEED_BAND_YELLOW_MAX_KPH, SPEED_EXTREME_OPERATION_THRESHOLD_KPH,
    };

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
        assert!(speed_bar_cells(100, 0).is_empty());
    }

    #[test]
    fn speed_band_boundaries() {
        assert_eq!(speed_band(0), SpeedBand::Green);
        assert_eq!(speed_band(SPEED_BAND_GREEN_MAX_KPH), SpeedBand::Green);
        assert_eq!(speed_band(SPEED_BAND_GREEN_MAX_KPH + 1), SpeedBand::Yellow);
        assert_eq!(speed_band(SPEED_BAND_YELLOW_MAX_KPH), SpeedBand::Yellow);
        assert_eq!(speed_band(SPEED_BAND_YELLOW_MAX_KPH + 1), SpeedBand::Red);
        assert_eq!(
            speed_band(SPEED_EXTREME_OPERATION_THRESHOLD_KPH),
            SpeedBand::Red
        );
    }

    #[test]
    fn zoned_cells_cover_green_yellow_red_on_full_scale_bar() {
        let cells = speed_bar_cells(SPEED_EXTREME_OPERATION_THRESHOLD_KPH, 16);
        assert_eq!(cells.len(), 16);
        assert!(cells.iter().all(|c| c.filled));
        assert!(cells.iter().any(|c| c.band == SpeedBand::Green));
        assert!(cells.iter().any(|c| c.band == SpeedBand::Yellow));
        assert!(cells.iter().any(|c| c.band == SpeedBand::Red));
        // First cells are green; last cell is red (160).
        assert_eq!(cells[0].band, SpeedBand::Green);
        assert_eq!(cells[15].band, SpeedBand::Red);
    }

    #[test]
    fn empty_cells_keep_zone_band() {
        let cells = speed_bar_cells(0, 16);
        assert!(cells.iter().all(|c| !c.filled));
        assert!(cells.iter().any(|c| c.band == SpeedBand::Red));
    }
}
