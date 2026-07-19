//! Structured pane lines (semantic styles; no Ratatui).

use super::fit_line;
use common::vehicle_physics::{SpeedBand, SpeedBarCell};
use unicode_width::UnicodeWidthStr;

/// Stable identity of a pane row (whole-line policy later).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineRole {
    Standby,
    Notice,
    Speed,
    Visibility,
    Weather,
    EngineerState,
    EngineerEvent,
    EngineerRob,
    EngineerHeading,
    EngineerAssembly,
    LedgerRow,
}

/// Semantic style token — mapped to Ratatui colours only in `main`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SegmentStyle {
    Default,
    Mute,
    ZoneGreen,
    ZoneYellow,
    ZoneRed,
}

impl SegmentStyle {
    pub fn from_speed_band(band: SpeedBand) -> Self {
        match band {
            SpeedBand::Green => Self::ZoneGreen,
            SpeedBand::Yellow => Self::ZoneYellow,
            SpeedBand::Red => Self::ZoneRed,
        }
    }
}

/// Inline content for one segment of a [`PaneLine`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SegmentContent {
    Text(String),
    SpeedBar { cells: Vec<SpeedBarCell> },
    /// Reserved: visibility low/high boxes (not emitted yet).
    #[allow(dead_code)]
    Swatch,
    /// Reserved: weather glyphs (not emitted yet).
    #[allow(dead_code)]
    Icon,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    pub style: SegmentStyle,
    pub content: SegmentContent,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneLine {
    pub role: LineRole,
    pub segments: Vec<Segment>,
}

impl PaneLine {
    pub fn plain(role: LineRole, text: impl Into<String>) -> Self {
        Self {
            role,
            segments: vec![Segment {
                style: SegmentStyle::Default,
                content: SegmentContent::Text(text.into()),
            }],
        }
    }

    pub fn plain_fitted(role: LineRole, text: &str, width: usize) -> Self {
        Self::plain(role, fit_line(text, width))
    }

    /// Flatten to a single string (tests / width checks).
    pub fn text(&self) -> String {
        let mut out = String::new();
        for seg in &self.segments {
            match &seg.content {
                SegmentContent::Text(s) => out.push_str(s),
                SegmentContent::SpeedBar { cells } => {
                    for c in cells {
                        out.push(if c.filled { '|' } else { '.' });
                    }
                }
                SegmentContent::Swatch | SegmentContent::Icon => {}
            }
        }
        out
    }

    pub fn display_width(&self) -> usize {
        self.text().width()
    }

    /// Pad with trailing spaces so the line occupies exactly `width` columns.
    pub fn pad_to_width(mut self, width: usize) -> Self {
        if width == 0 {
            return self;
        }
        let used = self.display_width();
        if used < width {
            self.segments.push(Segment {
                style: SegmentStyle::Mute,
                content: SegmentContent::Text(" ".repeat(width - used)),
            });
        }
        self
    }
}
