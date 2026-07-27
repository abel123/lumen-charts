use super::{AXIS_COLOR, BG_COLOR};
use crate::chart_state::ChartState;
use crate::tick_marks::{generate_price_ticks, TickMark};
use crate::widget::{snap_x, snap_y, IcedBackend};

pub fn draw_background<'a>(b: &mut IcedBackend<'a>, layout: &crate::chart_model::ChartLayout) {
    b.fill_rect(
        0.0,
        0.0,
        layout.width as f64,
        layout.height as f64,
        BG_COLOR,
    );
}

/// Draw grid lines and pane borders for a SINGLE pane.
///
/// Margins apply only to the outer edges of the whole chart, not
/// between panes. The border policy is therefore:
///
/// * **Top border** — only drawn for the first pane (pane 0), so the
///   chart has a single top edge.
/// * **Bottom border** — only drawn for the last pane, so the chart
///   has a single bottom edge.
/// * **Right border** — drawn for every pane at the same x coordinate
///   (the right edge of the plot area). This is what gives the
///   Y-axis gutter its visual boundary.
/// * **Left border** — never drawn (the chart's left margin handles
///   the left edge once, and the plot area starts at that margin).
///
/// In single-canvas mode the caller iterates `render_pane` for every
/// pane, assembling the full grid pane by pane. In multi-canvas mode
/// each canvas draws exactly what it needs.
pub fn draw_pane_grid<'a>(
    pane_idx: usize,
    b: &mut IcedBackend<'a>,
    state: &ChartState,
    time_ticks: &[TickMark],
) {
    let grid = &state.options.grid;
    if !grid.visible {
        return;
    }

    let grid_color = grid.color;
    let sf = state.layout.scale_factor;
    let num_panes = state.panes.len();

    let pane = match state.panes.get(pane_idx) {
        Some(p) => p,
        None => return,
    };
    let r = &pane.layout_rect;

    const EXTEND: f64 = 2.0;
    let grid_top = (r.y as f64) - EXTEND;
    let grid_bottom = (r.y + r.height) as f64 + EXTEND;
    for tick in time_ticks {
        let x = snap_x(tick.coord as f64, sf);
        b.stroke_line(x, grid_top, x, grid_bottom, grid_color, 1.0);
    }

    let price_ticks = generate_price_ticks(&pane.price_scale, r);
    for tick in &price_ticks {
        let y = snap_y(tick.coord as f64, sf);
        b.stroke_line(r.x as f64, y, (r.x + r.width) as f64, y, grid_color, 1.0);
    }

    let right_edge = state.layout.width as f64;
    let is_first = pane_idx == 0;
    let is_last = pane_idx == num_panes.saturating_sub(1);

    b.stroke_line(
        r.x as f64,
        0.0,
        r.x as f64,
        (r.y + r.height) as f64,
        AXIS_COLOR,
        1.0,
    );

    if is_first {
        b.stroke_line(0.0, r.y as f64, right_edge, r.y as f64, AXIS_COLOR, 1.0);
    }

    if is_last {
        b.stroke_line(
            0.0,
            (r.y + r.height) as f64,
            right_edge,
            (r.y + r.height) as f64,
            AXIS_COLOR,
            1.0,
        );
    }

    b.stroke_line(
        right_edge,
        r.y as f64,
        right_edge,
        (r.y + r.height) as f64,
        AXIS_COLOR,
        1.0,
    );
}
