use super::{BEAR_COLOR, BULL_COLOR};
use crate::chart::chart_model::OhlcBar;
use crate::chart::chart_state::ChartState;
use crate::chart::color::Color;
use crate::chart::series::{
    AreaSeriesOptions, BaselineSeriesOptions, CandlestickOptions, HistogramDataPoint,
    HistogramSeriesOptions, LineDataPoint, LineSeriesOptions, LineType,
};
use crate::widget::{snap_x, snap_y, IcedBackend};

pub fn draw_ohlc_bars<'a>(b: &mut IcedBackend<'a>, pane_index: usize, state: &ChartState) {
    let pane = &state.panes[pane_index];
    let plot_area = &pane.layout_rect;
    let bar_width = state.time_scale.bar_spacing * 0.3;
    let line_width = 1.5;
    let sf = state.layout.scale_factor;

    let (first, last) = state.time_scale.visible_range(plot_area.width);
    let first = first.saturating_sub(1);
    let last = (last + 1).min(state.data.bars.len());

    for i in first..last {
        let bar = &state.data.bars[i];
        let x = snap_x(state.time_scale.index_to_x(i, plot_area) as f64, sf);

        if x < (plot_area.x - bar_width) as f64
            || x > (plot_area.x + plot_area.width + bar_width) as f64
        {
            continue;
        }

        let high_y = pane.price_scale.price_to_y(bar.high, plot_area) as f64;
        let low_y = pane.price_scale.price_to_y(bar.low, plot_area) as f64;
        let open_y = snap_y(pane.price_scale.price_to_y(bar.open, plot_area) as f64, sf);
        let close_y = snap_y(pane.price_scale.price_to_y(bar.close, plot_area) as f64, sf);

        let color = if bar.close >= bar.open {
            BULL_COLOR
        } else {
            BEAR_COLOR
        };

        b.stroke_line(x, high_y, x, low_y, color, line_width);
        b.stroke_line(x - bar_width as f64, open_y, x, open_y, color, line_width);
        b.stroke_line(x, close_y, x + bar_width as f64, close_y, color, line_width);
    }
}

pub fn draw_candlestick_bars<'a>(b: &mut IcedBackend<'a>, pane_index: usize, state: &ChartState) {
    draw_candlestick_bars_data(
        b,
        pane_index,
        state,
        &state.data.bars,
        &CandlestickOptions::default(),
    );
}

/// Draw candlestick bars from arbitrary bar data (for multi-series support)
pub fn draw_candlestick_bars_data<'a>(
    b: &mut IcedBackend<'a>,
    pane_index: usize,
    state: &ChartState,
    bars: &[OhlcBar],
    opts: &CandlestickOptions,
) {
    let pane = &state.panes[pane_index];
    let plot_area = &pane.layout_rect;
    let bar_width = (state.time_scale.bar_spacing * 0.7).max(1.0);

    let (vis_first, vis_last) = state.time_scale.visible_range(plot_area.width);

    for bar in bars {
        let bar_idx = match state.time_index_map.get(&bar.time) {
            Some(&idx) => idx,
            None => continue,
        };
        if bar_idx + 1 < vis_first || bar_idx > vis_last + 1 {
            continue;
        }
        let x = state.time_scale.index_to_x(bar_idx, plot_area) as f64;

        if x < (plot_area.x - bar_width) as f64
            || x > (plot_area.x + plot_area.width + bar_width) as f64
        {
            continue;
        }

        let open_y = pane.price_scale.price_to_y(bar.open, plot_area) as f64;
        let close_y = pane.price_scale.price_to_y(bar.close, plot_area) as f64;
        let high_y = pane.price_scale.price_to_y(bar.high, plot_area) as f64;
        let low_y = pane.price_scale.price_to_y(bar.low, plot_area) as f64;

        let is_bull = bar.close >= bar.open;
        let body_top = open_y.min(close_y);
        let body_bottom = open_y.max(close_y);
        let body_height = (body_bottom - body_top).max(1.0);
        let half_w = bar_width as f64 / 2.0;

        let body_color = if is_bull {
            opts.up_color
        } else {
            opts.down_color
        };
        let wick_color = if is_bull {
            opts.wick_up_color
        } else {
            opts.wick_down_color
        };

        b.stroke_line(x, high_y, x, low_y, wick_color, 1.0);

        if opts.hollow && is_bull {
            b.stroke_line(x - half_w, body_top, x + half_w, body_top, body_color, 1.0);
            b.stroke_line(
                x - half_w,
                body_bottom,
                x + half_w,
                body_bottom,
                body_color,
                1.0,
            );
            b.stroke_line(
                x - half_w,
                body_top,
                x - half_w,
                body_bottom,
                body_color,
                1.0,
            );
            b.stroke_line(
                x + half_w,
                body_top,
                x + half_w,
                body_bottom,
                body_color,
                1.0,
            );
        } else {
            b.fill_rect(
                x - half_w,
                body_top,
                bar_width as f64,
                body_height,
                body_color,
            );
        }
    }
}

/// Draw a line series from OHLC close prices (for primary series)
pub fn draw_line_series_from_ohlc<'a>(
    b: &mut IcedBackend<'a>,
    pane_index: usize,
    state: &ChartState,
) {
    let pane = &state.panes[pane_index];
    let plot_area = &pane.layout_rect;

    let (first, last) = state.time_scale.visible_range(plot_area.width);
    let first = first.saturating_sub(1);
    let last = (last + 1).min(state.data.bars.len());

    if first >= last {
        return;
    }

    let mut points: Vec<(f64, f64)> = Vec::with_capacity(last - first);
    for i in first..last {
        let bar = &state.data.bars[i];
        let x = state.time_scale.index_to_x(i, plot_area) as f64;
        let y = pane.price_scale.price_to_y(bar.close, plot_area) as f64;
        points.push((x, y));
    }

    log::info!("points: {:?}", points);
    b.stroke_path(&points, BULL_COLOR, 2.0);
}

/// Draw a line series from LineDataPoint data
pub fn draw_line_series<'a>(
    b: &mut IcedBackend<'a>,
    pane_index: usize,
    state: &ChartState,
    line_points: &[LineDataPoint],
    opts: &LineSeriesOptions,
) {
    let pane = &state.panes[pane_index];
    let plot_area = &pane.layout_rect;

    let (vis_first, vis_last) = state.time_scale.visible_range(plot_area.width);

    let mut indexed_points: Vec<(usize, f64, f64)> = Vec::with_capacity(line_points.len());
    for pt in line_points {
        let bar_idx = match state.time_index_map.get(&pt.time) {
            Some(&idx) => idx,
            None => continue,
        };
        if bar_idx + 1 < vis_first || bar_idx > vis_last + 1 {
            continue;
        }
        let x = state.time_scale.index_to_x(bar_idx, plot_area) as f64;
        let y = pane.price_scale.price_to_y(pt.value, plot_area) as f64;
        indexed_points.push((bar_idx, x, y));
    }

    let color = opts.color;
    let width = opts.line_width as f64;
    let line_type = opts.line_type;

    let mut segment: Vec<(f64, f64)> = Vec::new();
    let mut prev_idx: Option<usize> = None;

    for &(bar_idx, x, y) in &indexed_points {
        if let Some(prev) = prev_idx {
            if bar_idx > prev + 1 {
                flush_line_segment(b, &segment, color, width, line_type);
                segment.clear();
            }
        }
        segment.push((x, y));
        prev_idx = Some(bar_idx);
    }
    flush_line_segment(b, &segment, color, width, line_type);

    if opts.point_markers_visible && indexed_points.len() < 200 {
        let radius = opts.point_markers_radius as f64;
        for &(_, px, py) in &indexed_points {
            b.fill_circle(px, py, radius, color);
        }
    }
}

/// Flush a line segment with the appropriate interpolation (LineType).
pub fn flush_line_segment<'a>(
    b: &mut IcedBackend<'a>,
    segment: &[(f64, f64)],
    color: Color,
    width: f64,
    line_type: LineType,
) {
    if segment.len() < 2 {
        return;
    }
    match line_type {
        LineType::Simple => {
            b.stroke_path(segment, color, width);
        }
        LineType::WithSteps => {
            let mut stepped: Vec<(f64, f64)> = Vec::with_capacity(segment.len() * 2);
            stepped.push(segment[0]);
            for i in 1..segment.len() {
                stepped.push((segment[i].0, segment[i - 1].1));
                stepped.push(segment[i]);
            }
            b.stroke_path(&stepped, color, width);
        }
        LineType::Curved => {
            let mut curved: Vec<(f64, f64)> = Vec::with_capacity(segment.len() * 8);
            for i in 0..segment.len() - 1 {
                let p0 = if i > 0 { segment[i - 1] } else { segment[i] };
                let p1 = segment[i];
                let p2 = segment[i + 1];
                let p3 = if i + 2 < segment.len() {
                    segment[i + 2]
                } else {
                    segment[i + 1]
                };

                if i == 0 {
                    curved.push(p1);
                }

                let steps = 8;
                for s in 1..=steps {
                    let t = s as f64 / steps as f64;
                    let t2 = t * t;
                    let t3 = t2 * t;
                    let x = 0.5
                        * (2.0 * p1.0
                            + (-p0.0 + p2.0) * t
                            + (2.0 * p0.0 - 5.0 * p1.0 + 4.0 * p2.0 - p3.0) * t2
                            + (-p0.0 + 3.0 * p1.0 - 3.0 * p2.0 + p3.0) * t3);
                    let y = 0.5
                        * (2.0 * p1.1
                            + (-p0.1 + p2.1) * t
                            + (2.0 * p0.1 - 5.0 * p1.1 + 4.0 * p2.1 - p3.1) * t2
                            + (-p0.1 + 3.0 * p1.1 - 3.0 * p2.1 + p3.1) * t3);
                    curved.push((x, y));
                }
            }
            b.stroke_path(&curved, color, width);
        }
    }
}

/// Helper to convert OHLC to LineDataPoints
pub fn ohlc_to_line_points(bars: &[OhlcBar]) -> Vec<LineDataPoint> {
    bars.iter()
        .map(|b| LineDataPoint {
            time: b.time,
            value: b.close,
        })
        .collect()
}

/// Draw an area series (filled gradient below a line)
pub fn draw_area_series<'a>(
    b: &mut IcedBackend<'a>,
    pane_index: usize,
    state: &ChartState,
    line_points: &[LineDataPoint],
    opts: &AreaSeriesOptions,
) {
    let pane = &state.panes[pane_index];
    let plot_area = &pane.layout_rect;

    let (vis_first, vis_last) = state.time_scale.visible_range(plot_area.width);

    let mut indexed_points: Vec<(usize, f64, f64)> = Vec::with_capacity(line_points.len());
    for pt in line_points {
        let bar_idx = match state.time_index_map.get(&pt.time) {
            Some(&idx) => idx,
            None => continue,
        };
        if bar_idx + 1 < vis_first || bar_idx > vis_last + 1 {
            continue;
        }
        let x = state.time_scale.index_to_x(bar_idx, plot_area) as f64;
        let y = pane.price_scale.price_to_y(pt.value, plot_area) as f64;
        indexed_points.push((bar_idx, x, y));
    }

    let bottom_y = (plot_area.y + plot_area.height) as f64;

    let render_segment = |b: &mut IcedBackend<'a>, segment: &[(f64, f64)]| {
        if segment.len() < 2 {
            return;
        }
        let mut fill_pts: Vec<(f64, f64)> = segment.to_vec();
        if let Some(&(last_x, _)) = fill_pts.last() {
            fill_pts.push((last_x, bottom_y));
        }
        if let Some(&(first_x, _)) = segment.first() {
            fill_pts.push((first_x, bottom_y));
        }
        let lowest_y = segment
            .iter()
            .map(|(_, y)| *y)
            .fold(f64::INFINITY, f64::min);

        b.fill_path_gradient(
            &fill_pts,
            lowest_y,
            bottom_y,
            &[(opts.top_color, 0.0), (opts.bottom_color, 1.0)],
        );
        b.stroke_path(segment, opts.line_color, opts.line_width as f64);
    };

    let mut segment: Vec<(f64, f64)> = Vec::new();
    let mut prev_idx: Option<usize> = None;

    for &(bar_idx, x, y) in &indexed_points {
        if let Some(prev) = prev_idx {
            if bar_idx > prev + 1 {
                render_segment(b, &segment);
                segment.clear();
            }
        }
        segment.push((x, y));
        prev_idx = Some(bar_idx);
    }
    render_segment(b, &segment);
}

/// Draw a baseline series (filled areas above and below a baseline)
pub fn draw_baseline_series<'a>(
    b: &mut IcedBackend<'a>,
    pane_index: usize,
    state: &ChartState,
    line_points: &[LineDataPoint],
    opts: &BaselineSeriesOptions,
) {
    let pane = &state.panes[pane_index];
    let plot_area = &pane.layout_rect;

    let (vis_first, vis_last) = state.time_scale.visible_range(plot_area.width);

    let base_y = pane.price_scale.price_to_y(opts.base_value, plot_area) as f64;

    let mut indexed_points: Vec<(usize, f64, f64)> = Vec::with_capacity(line_points.len());
    for pt in line_points {
        let bar_idx = match state.time_index_map.get(&pt.time) {
            Some(&idx) => idx,
            None => continue,
        };
        if bar_idx + 1 < vis_first || bar_idx > vis_last + 1 {
            continue;
        }
        let x = state.time_scale.index_to_x(bar_idx, plot_area) as f64;
        let y = pane.price_scale.price_to_y(pt.value, plot_area) as f64;
        indexed_points.push((bar_idx, x, y));
    }

    let render_baseline_segment = |b: &mut IcedBackend<'a>, seg: &[(f64, f64)]| {
        if seg.len() < 2 {
            return;
        }

        let mut top_fill: Vec<(f64, f64)> = Vec::new();
        let mut bottom_fill: Vec<(f64, f64)> = Vec::new();
        for &(x, y) in seg {
            top_fill.push((x, y.min(base_y)));
            bottom_fill.push((x, y.max(base_y)));
        }

        if !top_fill.is_empty() {
            let mut pts = top_fill.clone();
            if let Some(&(lx, _)) = pts.last() {
                pts.push((lx, base_y));
            }
            if let Some(&(fx, _)) = pts.first() {
                pts.push((fx, base_y));
            }
            let min_y = pts.iter().map(|(_, y)| *y).fold(f64::INFINITY, f64::min);
            b.fill_path_gradient(
                &pts,
                min_y,
                base_y,
                &[
                    (opts.top_fill_color, 0.0),
                    (
                        Color([
                            opts.top_fill_color[0],
                            opts.top_fill_color[1],
                            opts.top_fill_color[2],
                            0.0,
                        ]),
                        1.0,
                    ),
                ],
            );
        }

        if !bottom_fill.is_empty() {
            let mut pts = Vec::new();
            if let Some(&(fx, _)) = bottom_fill.first() {
                pts.push((fx, base_y));
            }
            pts.extend_from_slice(&bottom_fill);
            if let Some(&(lx, _)) = bottom_fill.last() {
                pts.push((lx, base_y));
            }
            let max_y = pts
                .iter()
                .map(|(_, y)| *y)
                .fold(f64::NEG_INFINITY, f64::max);
            b.fill_path_gradient(
                &pts,
                base_y,
                max_y,
                &[
                    (opts.bottom_fill_color, 0.0),
                    (
                        Color([
                            opts.bottom_fill_color[0],
                            opts.bottom_fill_color[1],
                            opts.bottom_fill_color[2],
                            0.0,
                        ]),
                        1.0,
                    ),
                ],
            );
        }

        for i in 0..seg.len().saturating_sub(1) {
            let (x0, y0) = seg[i];
            let (x1, y1) = seg[i + 1];
            let mid_y = (y0 + y1) / 2.0;
            let color = if mid_y <= base_y {
                opts.top_line_color
            } else {
                opts.bottom_line_color
            };
            b.stroke_line(x0, y0, x1, y1, color, opts.line_width as f64);
        }
    };

    let mut segment: Vec<(f64, f64)> = Vec::new();
    let mut prev_idx: Option<usize> = None;

    for &(bar_idx, x, y) in &indexed_points {
        if let Some(prev) = prev_idx {
            if bar_idx > prev + 1 {
                render_baseline_segment(b, &segment);
                segment.clear();
            }
        }
        segment.push((x, y));
        prev_idx = Some(bar_idx);
    }
    render_baseline_segment(b, &segment);
}

/// Draw a histogram series — vertical bars from base to value
pub fn draw_histogram_series<'a>(
    b: &mut IcedBackend<'a>,
    pane_index: usize,
    state: &ChartState,
    points: &[HistogramDataPoint],
    opts: &HistogramSeriesOptions,
) {
    let pane = &state.panes[pane_index];
    let plot_area = &pane.layout_rect;
    let bar_width = (state.time_scale.bar_spacing * 0.6).max(1.0);

    let base_y = pane.price_scale.price_to_y(opts.base, plot_area) as f64;

    let (vis_first, vis_last) = state.time_scale.visible_range(plot_area.width);

    for pt in points {
        let bar_idx = match state.time_index_map.get(&pt.time) {
            Some(&idx) => idx,
            None => continue,
        };
        if bar_idx + 1 < vis_first || bar_idx > vis_last + 1 {
            continue;
        }
        let x = state.time_scale.index_to_x(bar_idx, plot_area) as f64;
        let val_y = pane.price_scale.price_to_y(pt.value, plot_area) as f64;

        if x < plot_area.x as f64 || x > (plot_area.x + plot_area.width) as f64 {
            continue;
        }

        let color = if let Some(c) = pt.color {
            c
        } else {
            opts.color
        };

        let half_w = bar_width as f64 / 2.0;
        let top = val_y.min(base_y);
        let height = (val_y - base_y).abs().max(1.0);
        b.fill_rect(x - half_w, top, bar_width as f64, height, color);
    }
}
