use settings::{RegisterSetting, Settings};

const DEFAULT_LANE_WIDTH: f32 = 16.0;
/// Floor for `lane_width`, shared with `scaled_lane_width`'s floor on the
/// scaled result so the two can't drift apart.
pub(crate) const MIN_LANE_WIDTH: f32 = 4.0;
const DEFAULT_ROW_HEIGHT: f32 = 0.0;
const DEFAULT_ZOOM: f32 = 1.0;

#[derive(Debug, Clone, PartialEq, RegisterSetting)]
pub struct GitGraphSettings {
    /// Base horizontal spacing between lanes, in pixels (floored at `MIN_LANE_WIDTH`).
    pub lane_width: f32,
    /// Extra per-row height in pixels added on top of the font-derived height.
    pub row_height: f32,
    /// Combined zoom multiplier for lane width and row height.
    pub zoom: f32,
    /// Fixed graph-area width in pixels for the sidebar panel; `None` = auto.
    pub graph_width: Option<f32>,
}

impl Settings for GitGraphSettings {
    fn from_settings(content: &settings::SettingsContent) -> Self {
        let git_graph = content.git_graph.clone().unwrap_or_default();
        Self {
            lane_width: git_graph
                .lane_width
                .unwrap_or(DEFAULT_LANE_WIDTH)
                .max(MIN_LANE_WIDTH),
            row_height: git_graph.row_height.unwrap_or(DEFAULT_ROW_HEIGHT),
            zoom: git_graph.zoom.unwrap_or(DEFAULT_ZOOM),
            graph_width: git_graph.graph_width,
        }
    }
}
