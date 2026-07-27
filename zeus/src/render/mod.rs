use crate::chart::chart_state::ChartState;
use crate::chart::color::{Color, Palette};
use crate::chart::series::{SeriesData, SeriesType};
use crate::chart::tick_marks::{generate_price_ticks, generate_time_ticks};
use crate::widget::{snap_y, IcedBackend};

const LABEL_FONT_SIZE: f64 = 11.0;

const BG_COLOR: Color = Palette::Background.color();
const AXIS_COLOR: Color = Palette::Axis.color();
const BULL_COLOR: Color = Palette::Bull.color();
const BEAR_COLOR: Color = Palette::Bear.color();
const TEXT_COLOR: Color = Palette::Text.color();
const CROSSHAIR_COLOR: Color = Palette::Crosshair.color();
const WHITE: Color = Palette::White.color();

mod axes;
mod crosshair;
mod grid;
mod overlays;
mod series;

#[allow(unused_imports)]
pub use axes::{draw_x_axis, draw_y_axis};
#[allow(unused_imports)]
pub use crosshair::{draw_crosshair, draw_crosshair_for_pane};
#[allow(unused_imports)]
pub use grid::{draw_background, draw_pane_grid};
#[allow(unused_imports)]
pub use overlays::{
    draw_last_value_label, draw_last_value_marker, draw_price_line_labels, draw_price_lines,
    draw_series_markers, draw_watermark,
};
#[allow(unused_imports)]
pub use series::{
    draw_area_series, draw_baseline_series, draw_candlestick_bars, draw_candlestick_bars_data,
    draw_histogram_series, draw_line_series, draw_line_series_from_ohlc, draw_ohlc_bars,
    flush_line_segment, ohlc_to_line_points,
};

/// Render the "bottom" scene: background, grid, series, axes, overlays.
/// This is the expensive part that should be cached when only the crosshair moves.
pub fn render_bottom_scene<'a>(b: &mut IcedBackend<'a>, state: &ChartState) {
    if state.data.bars.is_empty() {
        return;
    }

    let layout = &state.layout;
    let sf = layout.scale_factor;
    b.set_scale(sf, sf);

    let pane_count = state.panes.len();
    for pane_idx in 0..pane_count {
        render_pane(pane_idx, b, state);
    }

    let time_ticks =
        generate_time_ticks(&state.data.bars, &state.time_scale, &state.layout.plot_area);
    draw_y_axis(b, state, layout);
    draw_x_axis(b, &time_ticks, layout);

    draw_price_line_labels(b, state);
    draw_last_value_label(b, state);
}

/// Per-pane rendering — draws everything that lives INSIDE the pane
/// rectangle. Used both by [`render_bottom_scene`] (which wraps this in
/// `with_clip`) and by the multi-canvas widget tree
/// (`paint_pane_to_iced_frame`) where each canvas already has its own
/// widget bounds.
///
/// # Drawing order
///
/// 1. Fill the pane's background (opaque, so content doesn't bleed
///    through from panes above/below in the single-canvas path).
/// 2. Draw grid lines and borders for this pane.
/// 3. Draw watermark.
/// 4. Draw series (primary + overlayed).
/// 5. Draw per-pane overlays (price lines, markers, last value).
///
/// This order ensures the grid is always visible on top of the
/// background, regardless of pane index.
pub fn render_pane<'a>(pane_idx: usize, b: &mut IcedBackend<'a>, state: &ChartState) {
    let layout = &state.layout;
    let time_ticks = generate_time_ticks(&state.data.bars, &state.time_scale, &layout.plot_area);

    let sf = layout.scale_factor;
    b.set_scale(sf, sf);

    draw_background(b, layout);

    draw_pane_grid(pane_idx, b, state, &time_ticks);

    if state.overlays.watermark.visible {
        draw_watermark(b, state);
    }

    if pane_idx == 0 {
        match state.active_series_type {
            SeriesType::Ohlc => draw_ohlc_bars(b, 0, state),
            SeriesType::Candlestick => draw_candlestick_bars(b, 0, state),
            SeriesType::Line => draw_line_series_from_ohlc(b, 0, state),
            SeriesType::Area => {
                let points = ohlc_to_line_points(&state.data.bars);
                let opts = crate::chart::series::AreaSeriesOptions::default();
                draw_area_series(b, 0, state, &points, &opts);
            }
            SeriesType::Baseline => {
                let points = ohlc_to_line_points(&state.data.bars);
                let opts = crate::chart::series::BaselineSeriesOptions::default();
                draw_baseline_series(b, 0, state, &points, &opts);
            }
            SeriesType::Histogram => {
                let points: Vec<crate::chart::series::HistogramDataPoint> = state
                    .data
                    .bars
                    .iter()
                    .map(|bar| crate::chart::series::HistogramDataPoint {
                        time: bar.time,
                        value: bar.close,
                        color: None,
                    })
                    .collect();
                let opts = crate::chart::series::HistogramSeriesOptions::default();
                draw_histogram_series(b, 0, state, &points, &opts);
            }
        }
    }

    for series in &state.series.series {
        if !series.visible || series.pane_index != pane_idx {
            continue;
        }
        match (&series.series_type, &series.data) {
            (SeriesType::Line, SeriesData::Line(pts)) => {
                draw_line_series(b, pane_idx, state, pts, &series.line_options);
            }
            (SeriesType::Area, SeriesData::Line(pts)) => {
                draw_area_series(b, pane_idx, state, pts, &series.area_options);
            }
            (SeriesType::Baseline, SeriesData::Line(pts)) => {
                draw_baseline_series(b, pane_idx, state, pts, &series.baseline_options);
            }
            (SeriesType::Candlestick, SeriesData::Ohlc(bars)) => {
                draw_candlestick_bars_data(b, pane_idx, state, bars, &series.candlestick_options);
            }
            (SeriesType::Histogram, SeriesData::Histogram(pts)) => {
                draw_histogram_series(b, pane_idx, state, pts, &series.histogram_options);
            }
            _ => {}
        }
    }

    if pane_idx == 0 {
        draw_price_lines(b, state, pane_idx);
        draw_series_markers(b, state);
        draw_last_value_marker(b, state, pane_idx);
    }
}

/// Render only the crosshair layer. This is cheap — just 2 dashed lines + labels.
pub fn render_crosshair_scene<'a>(b: &mut IcedBackend<'a>, state: &ChartState) {
    if !state.crosshair.visible {
        return;
    }
    let sf = state.layout.scale_factor;
    b.set_scale(sf, sf);
    draw_crosshair(b, state);
}

/// Render only the crosshair layer constrained to a single pane. Used by
/// the multi-canvas widget tree where each pane canvas draws its own
/// slice of the crosshair.
pub fn render_crosshair_for_pane<'a>(pane_idx: usize, b: &mut IcedBackend<'a>, state: &ChartState) {
    if !state.crosshair.visible {
        return;
    }
    let sf = state.layout.scale_factor;
    b.set_scale(sf, sf);
    draw_crosshair_for_pane(pane_idx, b, state);
}

/// Render the axis elements for a single pane in the multi-canvas
/// widget tree. Each pane canvas must draw its own portion of the
/// chart's axes because the canvas clips rendering to its own bounds.
///
/// Specifically:
/// - Y-axis gutter background + price labels for **this pane's** price scale
/// - Price line labels and last value label (pane 0 only)
/// - Crosshair price label on the Y-axis gutter (active pane only)
/// - X-axis time labels (bottom-most pane only)
pub fn render_pane_axes<'a>(pane_idx: usize, b: &mut IcedBackend<'a>, state: &ChartState) {
    let layout = &state.layout;
    let sf = layout.scale_factor;
    b.set_scale(sf, sf);

    let pane = match state.panes.get(pane_idx) {
        Some(p) => p,
        None => return,
    };
    let r = &pane.layout_rect;

    let gutter_x = (layout.plot_area.x + layout.plot_area.width) as f64;
    let gutter_w = layout.margins.right as f64;
    let gutter_top = (r.y as f64) - 2.0;
    let gutter_h = (r.height as f64) + 4.0;
    b.fill_rect(gutter_x, gutter_top, gutter_w, gutter_h, BG_COLOR);

    let price_ticks = generate_price_ticks(&pane.price_scale, r);
    let x_start = (layout.plot_area.x + layout.plot_area.width + 5.0) as f64;
    for tick in &price_ticks {
        let y = snap_y(tick.coord as f64, sf);
        b.draw_text(
            &format!("{:.2}", tick.value),
            x_start,
            y + 4.0,
            LABEL_FONT_SIZE,
            TEXT_COLOR,
        );
    }

    if state.crosshair.visible && state.active_pane == pane_idx {
        let y = snap_y(state.crosshair.y as f64, sf);
        if y >= r.y as f64 && y <= (r.y + r.height) as f64 {
            let label = format!("{:.2}", state.crosshair.price.unwrap_or(0.0));
            let label_x = (layout.plot_area.x + layout.plot_area.width + 2.0) as f64;
            let label_w = (layout.margins.right - 4.0) as f64;
            let label_h = 18.0;
            let label_y = y - label_h / 2.0;
            b.fill_rect(
                label_x,
                label_y,
                label_w,
                label_h,
                Palette::CrosshairLabelBg.color(),
            );
            b.draw_text(&label, label_x + 4.0, label_y + 13.0, 10.0, WHITE);
        }
    }

    if pane_idx == 0 {
        draw_price_line_labels(b, state);
        draw_last_value_label(b, state);
    }

    if pane_idx == state.panes.len().saturating_sub(1) {
        let time_ticks =
            generate_time_ticks(&state.data.bars, &state.time_scale, &layout.plot_area);
        draw_x_axis(b, &time_ticks, layout);
    }
}
