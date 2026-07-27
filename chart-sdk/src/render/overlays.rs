use super::{BEAR_COLOR, BULL_COLOR, LABEL_FONT_SIZE, WHITE};
use crate::chart_state::ChartState;
use crate::overlays::{LineStyle, MarkerPosition, MarkerShape};
use crate::widget::{snap_y, IcedBackend};

/// Draw price line indicators (horizontal lines) within a specific pane.
pub fn draw_price_lines<'a>(b: &mut IcedBackend<'a>, state: &ChartState, pane_idx: usize) {
    let pane = &state.panes[pane_idx];
    let plot = &pane.layout_rect;
    let sf = state.layout.scale_factor;

    for line in &state.overlays.price_lines {
        let y = pane.price_scale.price_to_y(line.price, plot);
        if y < plot.y || y > plot.y + plot.height {
            continue;
        }
        let y = snap_y(y as f64, sf);
        let color = line.color;

        match line.line_style {
            LineStyle::Dashed => {
                b.stroke_dashed_line(
                    plot.x as f64,
                    y,
                    (plot.x + plot.width) as f64,
                    y,
                    color,
                    line.line_width as f64,
                    6.0,
                    4.0,
                );
            }
            LineStyle::Dotted => {
                b.stroke_dashed_line(
                    plot.x as f64,
                    y,
                    (plot.x + plot.width) as f64,
                    y,
                    color,
                    line.line_width as f64,
                    2.0,
                    3.0,
                );
            }
            LineStyle::LargeDashed => {
                b.stroke_dashed_line(
                    plot.x as f64,
                    y,
                    (plot.x + plot.width) as f64,
                    y,
                    color,
                    line.line_width as f64,
                    6.0,
                    6.0,
                );
            }
            LineStyle::SparseDotted => {
                b.stroke_dashed_line(
                    plot.x as f64,
                    y,
                    (plot.x + plot.width) as f64,
                    y,
                    color,
                    line.line_width as f64,
                    1.0,
                    4.0,
                );
            }
            LineStyle::Solid => {
                b.stroke_line(
                    plot.x as f64,
                    y,
                    (plot.x + plot.width) as f64,
                    y,
                    color,
                    line.line_width as f64,
                );
            }
        }
    }
}

/// Draw price line labels in the Y-axis gutter (outside pane clip).
pub fn draw_price_line_labels<'a>(b: &mut IcedBackend<'a>, state: &ChartState) {
    let pane = &state.panes[0];
    let plot = &pane.layout_rect;
    let sf = state.layout.scale_factor;

    for line in &state.overlays.price_lines {
        if !line.label_visible {
            continue;
        }
        let y = pane.price_scale.price_to_y(line.price, plot);
        if y < plot.y || y > plot.y + plot.height {
            continue;
        }
        let y = snap_y(y as f64, sf);
        let color = line.color;

        let label_x = (plot.x + plot.width + 2.0) as f64;
        let label_w = b.measure_text(&line.label, LABEL_FONT_SIZE) + 8.0;
        let label_h = 16.0;
        let label_y = y - label_h / 2.0;

        b.fill_rect(label_x, label_y, label_w, label_h, color);
        b.draw_text(
            &line.label,
            label_x + 4.0,
            label_y + 12.0,
            LABEL_FONT_SIZE,
            WHITE,
        );
    }
}

pub fn draw_series_markers<'a>(b: &mut IcedBackend<'a>, state: &ChartState) {
    let plot = &state.layout.plot_area;
    let price_scale = &state.panes[0].price_scale;

    for marker in &state.overlays.markers {
        let bar_idx = match state
            .data
            .bars
            .binary_search_by_key(&marker.time, |bar| bar.time)
        {
            Ok(i) => i,
            Err(_) => continue,
        };

        let x = state.time_scale.index_to_x(bar_idx, plot) as f64;
        if x < plot.x as f64 || x > (plot.x + plot.width) as f64 {
            continue;
        }

        let bar = &state.data.bars[bar_idx];
        let marker_size = marker.size as f64;
        let color = marker.color;

        let y = match marker.position {
            MarkerPosition::AboveBar => {
                price_scale.price_to_y(bar.high, plot) as f64 - marker_size - 4.0
            }
            MarkerPosition::BelowBar => {
                price_scale.price_to_y(bar.low, plot) as f64 + marker_size + 4.0
            }
            MarkerPosition::AtPrice => price_scale.price_to_y(bar.close, plot) as f64,
        };

        match marker.shape {
            MarkerShape::ArrowUp => {
                let pts = [
                    (x, y - marker_size),
                    (x - marker_size * 0.6, y + marker_size * 0.3),
                    (x + marker_size * 0.6, y + marker_size * 0.3),
                ];
                b.fill_path(&pts, color);
            }
            MarkerShape::ArrowDown => {
                let pts = [
                    (x, y + marker_size),
                    (x - marker_size * 0.6, y - marker_size * 0.3),
                    (x + marker_size * 0.6, y - marker_size * 0.3),
                ];
                b.fill_path(&pts, color);
            }
            MarkerShape::Circle => {
                b.fill_circle(x, y, marker_size * 0.5, color);
            }
            MarkerShape::Square => {
                let half = marker_size * 0.5;
                b.fill_rect(x - half, y - half, marker_size, marker_size, color);
            }
        }

        if !marker.text.is_empty() {
            let text_y = match marker.position {
                MarkerPosition::AboveBar => y - marker_size - 2.0,
                MarkerPosition::BelowBar | MarkerPosition::AtPrice => {
                    y + marker_size + LABEL_FONT_SIZE + 2.0
                }
            };
            let text_w = b.measure_text(&marker.text, LABEL_FONT_SIZE);
            b.draw_text(
                &marker.text,
                x - text_w / 2.0,
                text_y,
                LABEL_FONT_SIZE,
                color,
            );
        }
    }
}

pub fn draw_watermark<'a>(b: &mut IcedBackend<'a>, state: &ChartState) {
    let wm = &state.overlays.watermark;
    let plot = &state.layout.plot_area;
    let font_size = wm.font_size as f64;

    for (i, line) in wm.text.lines().enumerate() {
        let x = (plot.x + plot.width / 2.0) as f64;
        let y = (plot.y + plot.height / 2.0) as f64 + (i as f64 * font_size * 1.2);
        let text_w = b.measure_text(line, font_size);
        b.draw_text(line, x - text_w / 2.0, y, font_size, wm.color);
    }
}

/// Draw last value dashed line within the specified pane (inside clip).
pub fn draw_last_value_marker<'a>(b: &mut IcedBackend<'a>, state: &ChartState, pane_idx: usize) {
    let pane = &state.panes[pane_idx];
    let plot = &pane.layout_rect;

    if let Some(last_bar) = state.data.bars.last() {
        let price = last_bar.close;
        let y = pane.price_scale.price_to_y(price, plot) as f64;

        if y < plot.y as f64 || y > (plot.y + plot.height) as f64 {
            return;
        }

        let color = if last_bar.close >= last_bar.open {
            BULL_COLOR
        } else {
            BEAR_COLOR
        };

        b.stroke_dashed_line(
            plot.x as f64,
            y,
            (plot.x + plot.width) as f64,
            y,
            color,
            1.0,
            4.0,
            3.0,
        );
    }
}

/// Draw last value label in the Y-axis gutter (outside pane clip).
pub fn draw_last_value_label<'a>(b: &mut IcedBackend<'a>, state: &ChartState) {
    let pane = &state.panes[0];
    let plot = &pane.layout_rect;

    if let Some(last_bar) = state.data.bars.last() {
        let price = last_bar.close;
        let y = pane.price_scale.price_to_y(price, plot) as f64;

        if y < plot.y as f64 || y > (plot.y + plot.height) as f64 {
            return;
        }

        let color = if last_bar.close >= last_bar.open {
            BULL_COLOR
        } else {
            BEAR_COLOR
        };

        let label = format!("{:.2}", price);
        let label_w = b.measure_text(&label, LABEL_FONT_SIZE) + 12.0;
        let label_h = LABEL_FONT_SIZE + 6.0;
        let label_x = (plot.x + plot.width) as f64 + 2.0;
        let label_y = y - label_h / 2.0;

        b.fill_rect(label_x, label_y, label_w, label_h, color);
        b.draw_text(
            &label,
            label_x + 6.0,
            label_y + LABEL_FONT_SIZE,
            LABEL_FONT_SIZE,
            WHITE,
        );
    }
}
