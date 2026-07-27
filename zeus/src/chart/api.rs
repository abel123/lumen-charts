//! Lumen Charts — single-crate safe Rust API for the v5 chart model.
//!
//! This crate unifies the previous `lumen-charts-core` (engine) and
//! `lumen-charts-sdk` (v5 wrappers) into one crate. All chart engine
//! types are still reachable through the inner [`Chart`] struct and
//! through the re-exports below; the recommended high-level API mirrors
//! LWC v5 (`ChartApi`, `SeriesApi`, `PaneApi`, `ITimeScaleApi`,
//! `IPriceScaleApi`).
//!
//! # Example
//!
//! ```ignore
//! use lumen_charts_sdk::{ChartApi, SeriesDefinition, OhlcBar};
//!
//! let mut chart = ChartApi::new(800, 600, 1.0);
//! chart.set_data(bars);
//! let series = chart.add_series(SeriesDefinition::Candlestick);
//! series.set_ohlc_data(&mut chart, &bars);
//! let pane = chart.add_pane(0.3);
//! series.move_to_pane(&mut chart, &pane);
//! ```
//!
//! # Architecture
//!
//! - `pub mod chart_state`, `pub mod chart_model`, etc. — the chart engine
//!   (pure-Rust, no FFI).
//! - [`Chart`] — a thin wrapper around [`ChartState`].
//! - [`ChartApi`] — the v5 safe Rust wrapper that owns a [`Chart`] and
//!   exposes the high-level API.
//! - All state is accessed directly (Rust-to-Rust). There is **no C-ABI**,
//!   no `unsafe` in the public API, and no `std::ffi` types.
//!
//! Iced 0.14 integration lives in [`renderers::iced`]. The recommended
//! path is: `IcedChart::new(ChartApi::with_size(w, h, scale))` then call
//! `.canvas()` to embed in your Iced view.

// ── Chart engine modules ─────────────────────────────────────────────────

// ════════════════════════════════════════════════════════════════════════════
//  Chart — owns state + a renderer pipeline (safe Rust API)
// ════════════════════════════════════════════════════════════════════════════

use super::chart_model::{ChartData, OhlcBar};
use super::series::{HistogramDataPoint, LineDataPoint, PriceLineOptions, SeriesType};
use super::{chart_state::ChartState, invalidation::InvalidationLevel, series::Series};

/// Chart handle — owns the chart engine state.
///
/// `Chart` is a thin wrapper around [`ChartState`]. With only the Iced
/// backend in play, there's no per-chart renderer field to manage; the
/// [`IcedChart`] widget paints directly from `state` when Iced calls its
/// `Program::draw`. All chart mutations go through the safe methods on
/// [`ChartApi`].
///
/// The `state` field is `pub(crate)` so internal callers (the `IcedChart`
/// widget, etc.) can borrow it directly without needing a separate
/// accessor API.
pub struct Chart {
    pub(crate) state: ChartState,
}

// Helper trait alias — kept as a comment for the next reader.
impl Chart {
    /// Create a new chart with the given viewport size.
    ///
    /// The initial size is used to lay out the chart engine; the
    /// [`IcedChart`] widget will resize it to match Iced's actual canvas
    /// bounds on the first `Program::draw` call.
    pub fn new(width: u32, height: u32, scale_factor: f64) -> Self {
        let data = ChartData { bars: Vec::new() };
        let state = ChartState::new(data, width as f32, height as f32, scale_factor);
        Chart { state }
    }

    // ── Mutation helpers ────────────────────────────────────────────────

    fn invalidate_full(&mut self) {
        self.state.pending_mask.set_global(InvalidationLevel::Full);
    }

    fn invalidate_light(&mut self) {
        self.state.pending_mask.set_global(InvalidationLevel::Light);
    }

    // ── Public safe methods (no FFI, no `unsafe`) ───────────────────────

    /// Render the chart unconditionally.
    ///
    /// Marks the invalidation mask as consumed. The actual paint happens
    /// later when the Iced widget's `Program::draw` runs.
    pub fn render(&mut self) {
        let _ = self.state.consume_mask();
    }

    /// Render only if the invalidation mask says a redraw is needed.
    /// Returns true if a render was performed.
    pub fn render_if_needed(&mut self) -> bool {
        let mask = self.state.consume_mask();
        if !mask.needs_redraw() {
            self.state.skipped_render_count += 1;
            return false;
        }
        true
    }

    /// Resize the chart viewport.
    pub fn resize(&mut self, width: u32, height: u32, scale_factor: f64) {
        self.state.resize(width as f32, height as f32, scale_factor);
    }

    /// Handle a pointer/mouse move. Returns true if a redraw is needed.
    pub fn pointer_move(&mut self, x: f32, y: f32) -> bool {
        self.state.pointer_move(x, y)
    }

    /// Handle a pointer/mouse button press. Returns true if a redraw is needed.
    pub fn pointer_down(&mut self, x: f32, y: f32, button: u8) -> bool {
        self.state.pointer_down(x, y, button)
    }

    /// Handle a pointer/mouse button release. Returns true if a redraw is needed.
    pub fn pointer_up(&mut self, x: f32, y: f32, button: u8) -> bool {
        self.state.pointer_up(x, y, button)
    }

    /// Handle pointer leaving the chart area. Returns true if a redraw is needed.
    pub fn pointer_leave(&mut self) -> bool {
        self.state.pointer_leave()
    }

    /// Handle a scroll/wheel event. Returns true if a redraw is needed.
    pub fn scroll(&mut self, dx: f32, dy: f32) -> bool {
        self.state.scroll(dx, dy)
    }

    /// Handle a zoom event. Returns true if a redraw is needed.
    pub fn zoom(&mut self, factor: f32, center_x: f32) -> bool {
        self.state.zoom(factor, center_x)
    }

    /// Handle a pinch-to-zoom gesture. Returns true if a redraw is needed.
    pub fn pinch(&mut self, scale: f32, center_x: f32, center_y: f32) -> bool {
        self.state.pinch(scale, center_x, center_y)
    }

    /// Handle a keyboard key-down event. Returns true if a redraw is needed.
    pub fn key_down(&mut self, key_code: u32) -> bool {
        let key = super::chart_state::ChartKey::from_code(key_code);
        self.state.key_down(key)
    }

    /// Fit all data into the visible viewport.
    pub fn fit_content(&mut self) {
        self.state.fit_content();
    }

    /// Switch the primary series rendering type.
    /// `0`=OHLC, `1`=Candle, `2`=Line, `3`=Area, `4`=Hist, `5`=Baseline.
    pub fn set_series_type(&mut self, type_index: u32) {
        self.state.set_series_type(type_index);
    }

    /// Set OHLC bar data directly.
    pub fn set_data(&mut self, bars: Vec<OhlcBar>) {
        self.state.set_data(bars);
    }

    /// Set OHLC bar data from a flat array of (time, O, H, L, C) tuples.
    /// The slice length must be a multiple of 5.
    pub fn set_data_from_slice(&mut self, flat_data: &[f64]) {
        let count = flat_data.len() / 5;
        let bars: Vec<OhlcBar> = (0..count)
            .map(|i| {
                let base = i * 5;
                OhlcBar {
                    time: flat_data[base] as i64,
                    open: flat_data[base + 1],
                    high: flat_data[base + 2],
                    low: flat_data[base + 3],
                    close: flat_data[base + 4],
                }
            })
            .collect();
        self.state.set_data(bars);
    }

    /// Apply chart options from a JSON string. Returns `true` if applied.
    pub fn apply_options(&mut self, json: &str) -> bool {
        if json.is_empty() {
            return false;
        }
        if self.state.options.apply_json(json) {
            self.state.update_price_scale();
            self.invalidate_full();
            true
        } else {
            false
        }
    }

    /// Get current chart options as a JSON string.
    pub fn get_options(&self) -> String {
        serde_json::to_string(&self.state.options).unwrap_or_else(|_| "{}".to_string())
    }

    /// Set the crosshair to a specific price/time. Returns true if a redraw is needed.
    pub fn set_crosshair_position(&mut self, price: f64, time: i64, series_id: u32) -> bool {
        self.state.set_crosshair_position(price, time, series_id)
    }

    /// Clear the crosshair position. Returns true if a redraw is needed.
    pub fn clear_crosshair_position(&mut self) -> bool {
        self.state.clear_crosshair_position()
    }

    /// Convert a logical index to an X pixel coordinate.
    pub fn logical_to_coordinate(&self, logical: f64) -> f32 {
        self.state
            .time_scale
            .logical_to_x(logical as f32, &self.state.layout.plot_area)
    }

    /// Convert an X pixel coordinate to a logical index.
    pub fn coordinate_to_logical(&self, x: f32) -> f64 {
        self.state
            .time_scale
            .x_to_index(x, &self.state.layout.plot_area) as f64
    }

    /// Convert a price to a Y pixel coordinate in the given pane.
    pub fn price_to_coordinate(&self, pane_index: u32, price: f64) -> f32 {
        let pi = (pane_index as usize).min(self.state.panes.len().saturating_sub(1));
        self.state.panes[pi]
            .price_scale
            .price_to_y(price, &self.state.panes[pi].layout_rect)
    }

    /// Convert a Y pixel coordinate to a price in the given pane.
    pub fn coordinate_to_price(&self, pane_index: u32, y: f32) -> f64 {
        let pi = (pane_index as usize).min(self.state.panes.len().saturating_sub(1));
        self.state.panes[pi]
            .price_scale
            .y_to_price(y, &self.state.panes[pi].layout_rect)
    }

    /// Format a price using the chart's localization options.
    pub fn format_price(&self, price: f64) -> String {
        self.state.options.price_scale.format.format(price)
    }

    /// Format a timestamp using the chart's date localization settings.
    pub fn format_date(&self, timestamp: i64) -> String {
        super::formatters::format_date_custom(
            timestamp,
            &self.state.options.localization.date_format,
        )
    }

    /// Format a timestamp using the chart's time localization settings.
    pub fn format_time(&self, timestamp: i64) -> String {
        super::formatters::format_time_custom(
            timestamp,
            &self.state.options.localization.time_format,
        )
    }

    /// Number of primary OHLC bars.
    pub fn bar_count(&self) -> usize {
        self.state.bar_count()
    }

    // ── Time scale helpers (the v5 ITimeScaleApi surface) ──────────────

    /// Width of the time-scale area in logical pixels.
    pub fn time_scale_width(&self) -> f32 {
        self.state.layout.plot_area.width
    }

    /// Height of the time-scale area in logical pixels.
    pub fn time_scale_height(&self) -> f32 {
        self.state.layout.plot_area.height
    }

    /// Scroll to a specific bar position (fractional index from the right).
    /// `position > 0` = empty space at right, `< 0` = scrolled into history.
    pub fn time_scale_scroll_to_position(&mut self, position: f32) -> bool {
        self.state.time_scale.scroll_offset = position;
        self.state.time_scale.clamp_scroll();
        self.invalidate_light();
        true
    }

    /// Scroll so the last bar is visible at the right edge.
    pub fn time_scale_scroll_to_real_time(&mut self) -> bool {
        self.state.time_scale.scroll_offset = 0.0;
        self.invalidate_light();
        true
    }

    /// Get the visible time range as unix timestamps.
    /// Returns `None` if there is no data.
    pub fn time_scale_visible_range(&self) -> Option<(i64, i64)> {
        if self.state.data.bars.is_empty() {
            return None;
        }
        let pw = self.state.layout.plot_area.width;
        let (first, last) = self.state.time_scale.visible_range(pw);
        let start_time = self.state.data.bars.get(first).map(|b| b.time).unwrap_or(0);
        let end_time = self
            .state
            .data
            .bars
            .get(last.saturating_sub(1))
            .map(|b| b.time)
            .unwrap_or(0);
        Some((start_time, end_time))
    }

    /// Set the visible time range by start/end timestamps.
    pub fn time_scale_set_visible_range(&mut self, start_time: i64, end_time: i64) -> bool {
        if self.state.data.bars.is_empty() {
            return false;
        }
        let start_idx = self
            .state
            .data
            .bars
            .binary_search_by_key(&start_time, |b| b.time)
            .unwrap_or_else(|i| i);
        let end_idx = self
            .state
            .data
            .bars
            .binary_search_by_key(&end_time, |b| b.time)
            .unwrap_or_else(|i| i);
        let visible_bars = (end_idx as f32 - start_idx as f32).max(1.0);
        let plot_width = self.state.layout.plot_area.width;
        self.state.time_scale.bar_spacing = (plot_width / visible_bars).clamp(2.0, 50.0);
        let scroll = self
            .state
            .time_scale
            .scroll_offset_for_first(start_idx as f32, plot_width);
        self.state.time_scale.scroll_offset = scroll;
        self.state.time_scale.clamp_scroll();
        self.state.update_price_scale();
        self.invalidate_light();
        true
    }

    /// Get the visible logical range (first/last bar indices) as `f64`s.
    pub fn time_scale_visible_logical_range(&self) -> (f64, f64) {
        let pw = self.state.layout.plot_area.width;
        (
            self.state.time_scale.first_visible_index(pw) as f64,
            self.state.time_scale.last_visible_index(pw) as f64,
        )
    }

    /// Set the visible logical range by first/last bar indices.
    pub fn time_scale_set_visible_logical_range(&mut self, first: f64, last: f64) -> bool {
        let visible_bars = (last - first).max(1.0) as f32;
        let plot_width = self.state.layout.plot_area.width;
        self.state.time_scale.bar_spacing = (plot_width / visible_bars).clamp(2.0, 50.0);
        let scroll = self
            .state
            .time_scale
            .scroll_offset_for_first(first as f32, plot_width);
        self.state.time_scale.scroll_offset = scroll;
        self.state.time_scale.clamp_scroll();
        self.state.update_price_scale();
        self.invalidate_light();
        true
    }

    /// Reset the time scale to the default (fit content).
    pub fn time_scale_reset(&mut self) -> bool {
        self.state.fit_content();
        true
    }

    /// Apply time-scale options via JSON.
    pub fn time_scale_apply_options(&mut self, json: &str) -> bool {
        if json.is_empty() {
            return false;
        }
        let wrapper = format!("{{\"timeScale\":{}}}", json);
        if self.state.options.apply_json(&wrapper) {
            self.invalidate_full();
            true
        } else {
            false
        }
    }

    // ── Price scale helpers (the v5 IPriceScaleApi surface) ────────────

    /// Width of the price scale in pixels (right margin).
    pub fn price_scale_width(&self, _pane_index: u32) -> f32 {
        self.state.layout.margins.right
    }

    /// Get the price scale mode for a pane: `false` = Normal, `true` = Logarithmic.
    pub fn price_scale_get_mode(&self, pane_index: u32) -> PriceScaleModeKind {
        use super::price_scale::PriceScaleMode;
        match self.state.panes.get(pane_index as usize) {
            Some(p) => match p.price_scale.mode {
                PriceScaleMode::Logarithmic => PriceScaleModeKind::Logarithmic,
                PriceScaleMode::Normal => PriceScaleModeKind::Normal,
            },
            None => PriceScaleModeKind::Normal,
        }
    }

    /// Set the price scale mode for a pane.
    pub fn price_scale_set_mode(&mut self, pane_index: u32, mode: PriceScaleModeKind) -> bool {
        use super::price_scale::PriceScaleMode;
        if let Some(pane) = self.state.panes.get_mut(pane_index as usize) {
            pane.price_scale.mode = match mode {
                PriceScaleModeKind::Logarithmic => PriceScaleMode::Logarithmic,
                PriceScaleModeKind::Normal => PriceScaleMode::Normal,
            };
            self.invalidate_full();
            true
        } else {
            false
        }
    }

    /// Get whether auto-scale is enabled for a pane's price scale.
    pub fn price_scale_get_auto_scale(&self, pane_index: u32) -> bool {
        self.state
            .panes
            .get(pane_index as usize)
            .map(|p| p.price_scale.auto_scale)
            .unwrap_or(true)
    }

    /// Set whether auto-scale is enabled for a pane's price scale.
    pub fn price_scale_set_auto_scale(&mut self, pane_index: u32, enabled: bool) -> bool {
        if let Some(pane) = self.state.panes.get_mut(pane_index as usize) {
            pane.price_scale.auto_scale = enabled;
            if enabled {
                self.invalidate_full();
            }
            true
        } else {
            false
        }
    }

    /// Get the current visible price range `(min, max)` for a pane.
    pub fn price_scale_get_range(&self, pane_index: u32) -> Option<(f64, f64)> {
        self.state
            .panes
            .get(pane_index as usize)
            .map(|p| (p.price_scale.min_price, p.price_scale.max_price))
    }

    /// Apply price-scale options via JSON.
    pub fn price_scale_apply_options(&mut self, json: &str) -> bool {
        if json.is_empty() {
            return false;
        }
        let wrapper = format!("{{\"rightPriceScale\":{}}}", json);
        if self.state.options.apply_json(&wrapper) {
            self.state.update_price_scale();
            self.invalidate_full();
            true
        } else {
            false
        }
    }

    /// Get the current price-scale options as a JSON string.
    pub fn price_scale_get_options(&self, pane_index: u32) -> String {
        use super::price_scale::PriceScaleMode;
        let pi = (pane_index as usize).min(self.state.panes.len().saturating_sub(1));
        let ps = &self.state.panes[pi].price_scale;
        let mode_str = match ps.mode {
            PriceScaleMode::Normal => "normal",
            PriceScaleMode::Logarithmic => "logarithmic",
        };
        serde_json::json!({
            "mode": mode_str,
            "minPrice": ps.min_price,
            "maxPrice": ps.max_price,
            "width": self.state.layout.margins.right,
        })
        .to_string()
    }

    // ── Series operations targeting a specific series id ───────────────

    /// Apply options to a specific series from a JSON string.
    pub fn series_apply_options(&mut self, series_id: u32, json: &str) -> bool {
        if let Some(series) = self.state.series.get_mut(series_id) {
            let result = series.apply_options_json(json);
            if result {
                self.invalidate_full();
            }
            result
        } else {
            false
        }
    }

    /// Get the current options for a series as a JSON string.
    pub fn series_get_options(&self, series_id: u32) -> String {
        self.state
            .series
            .get(series_id)
            .map(|s| s.options_json())
            .unwrap_or_else(|| "{}".to_string())
    }

    /// Get the series type as an integer: `0`=Ohlc, `1`=Candlestick, `2`=Line,
    /// `3`=Area, `4`=Baseline, `5`=Histogram. Returns `-1` if the series
    /// does not exist.
    pub fn series_type_id(&self, series_id: u32) -> i32 {
        if let Some(s) = self.state.series.get(series_id) {
            match s.series_type {
                SeriesType::Ohlc => 0,
                SeriesType::Candlestick => 1,
                SeriesType::Line => 2,
                SeriesType::Area => 3,
                SeriesType::Baseline => 4,
                SeriesType::Histogram => 5,
            }
        } else {
            -1
        }
    }

    /// Set the z-order of a series within its pane.
    pub fn series_set_order(&mut self, series_id: u32, order: u32) -> bool {
        if self.state.series.set_series_order(series_id, order) {
            self.invalidate_full();
            true
        } else {
            false
        }
    }

    /// Set markers on a series from a JSON array string.
    /// Returns `true` on success.
    pub fn series_set_markers(&mut self, series_id: u32, markers_json: &str) -> bool {
        if self
            .state
            .overlays
            .set_markers_from_json(series_id, markers_json)
        {
            self.invalidate_full();
            true
        } else {
            false
        }
    }

    /// Get markers for a series as a JSON string.
    pub fn series_markers(&self) -> String {
        self.state.overlays.markers_to_json()
    }

    /// Number of bars in a logical index range for a specific series.
    pub fn series_bars_in_logical_range(&self, series_id: u32, from: f32, to: f32) -> u32 {
        if let Some(series) = self.state.series.get(series_id) {
            let from_idx = from.floor().max(0.0) as usize;
            let to_idx = to.ceil().max(0.0) as usize;
            let data_len = series.data.len();
            if from_idx >= data_len {
                return 0;
            }
            let to_idx = to_idx.min(data_len);
            (to_idx - from_idx) as u32
        } else {
            0
        }
    }

    /// Length of a specific series' data buffer.
    pub fn series_data_length(&self, series_id: u32) -> u32 {
        self.state
            .series
            .get(series_id)
            .map(|s| s.data.len() as u32)
            .unwrap_or(0)
    }
}

/// Linear vs Logarithmic price scale mode (v5 surface).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PriceScaleModeKind {
    Normal,
    Logarithmic,
}

// ════════════════════════════════════════════════════════════════════════════
//  v5: SeriesDefinition (unified addSeries entry point)
// ════════════════════════════════════════════════════════════════════════════

/// Defines the type of series to add.
///
/// v5 alignment: replaces per-type `addOhlcSeries`, `addCandlestickSeries`,
/// etc. with a single `chart.add_series(definition)` entry point.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SeriesDefinition {
    Ohlc,
    Candlestick,
    Line,
    Area,
    Histogram,
    Baseline { base_value: f64 },
}

// ════════════════════════════════════════════════════════════════════════════
//  v5: PaneApi
// ════════════════════════════════════════════════════════════════════════════

/// Handle to a chart pane (v5: index-based identity).
///
/// Pane indices shift when panes are removed. Pane 0 is the main
/// (always exists).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PaneApi {
    index: u32,
}

impl PaneApi {
    /// Get the pane index.
    pub fn pane_index(&self) -> u32 {
        self.index
    }

    /// Construct a `PaneApi` from a raw pane index. Caller must ensure
    /// the index is valid (i.e. `< chart.pane_count()`).
    pub fn from_index(index: u32) -> Self {
        Self { index }
    }
}

/// Minimum allowed `height_stretch` (prevents a pane from being squashed
/// to 0 by an aggressive drag).
pub const MIN_PANE_STRETCH: f32 = 0.05;
/// Maximum allowed `height_stretch` (prevents a pane from eating the
/// rest of the chart during drag).
pub const MAX_PANE_STRETCH: f32 = 20.0;

// ════════════════════════════════════════════════════════════════════════════
//  v5: SeriesApi
// ════════════════════════════════════════════════════════════════════════════

/// Handle to a chart series. Provides v5 `ISeriesApi` methods.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SeriesApi {
    id: u32,
}

impl SeriesApi {
    /// Get the series ID.
    pub fn id(&self) -> u32 {
        self.id
    }

    /// Get the series type.
    pub fn series_type(&self, chart: &ChartApi) -> Option<SeriesType> {
        chart.inner.state.series.get(self.id).map(|s| s.series_type)
    }

    // -- Data management --

    /// Set OHLC data for this series.
    pub fn set_ohlc_data(&self, chart: &mut ChartApi, data: &[OhlcBar]) {
        if let Some(series) = chart.inner.state.series.get_mut(self.id) {
            series.data.set_ohlc(data.to_vec());
            chart.invalidate();
        }
    }

    /// Set line/area/baseline data for this series.
    pub fn set_line_data(&self, chart: &mut ChartApi, data: &[LineDataPoint]) {
        if let Some(series) = chart.inner.state.series.get_mut(self.id) {
            series.data.set_line(data.to_vec());
            chart.invalidate();
        }
    }

    /// Set histogram data for this series.
    pub fn set_histogram_data(&self, chart: &mut ChartApi, data: &[HistogramDataPoint]) {
        if let Some(series) = chart.inner.state.series.get_mut(self.id) {
            series.data.set_histogram(data.to_vec());
            chart.invalidate();
        }
    }

    /// Update (or append) a single OHLC bar.
    pub fn update_ohlc(&self, chart: &mut ChartApi, bar: OhlcBar) {
        if let Some(series) = chart.inner.state.series.get_mut(self.id) {
            series.data.update_ohlc(bar);
            chart.invalidate();
        }
    }

    /// Update (or append) a single line/area/baseline point.
    pub fn update_line(&self, chart: &mut ChartApi, pt: LineDataPoint) {
        if let Some(series) = chart.inner.state.series.get_mut(self.id) {
            series.data.update_line(pt);
            chart.invalidate();
        }
    }

    /// Update (or append) a single histogram point.
    pub fn update_histogram(&self, chart: &mut ChartApi, pt: HistogramDataPoint) {
        if let Some(series) = chart.inner.state.series.get_mut(self.id) {
            series.data.update_histogram(pt);
            chart.invalidate();
        }
    }

    /// Number of data points.
    pub fn data_length(&self, chart: &ChartApi) -> usize {
        chart
            .inner
            .state
            .series
            .get(self.id)
            .map(|s| s.data.len())
            .unwrap_or(0)
    }

    /// Remove `count` items from the end of the series.
    pub fn pop(&self, chart: &mut ChartApi, count: usize) {
        if let Some(series) = chart.inner.state.series.get_mut(self.id) {
            series.data.pop(count);
            chart.invalidate();
        }
    }

    // -- Options --

    /// Apply a partial JSON options string (e.g. `{"color":[1,0,0,1]}`).
    pub fn apply_options(&self, chart: &mut ChartApi, json: &str) -> bool {
        chart.inner.series_apply_options(self.id, json)
    }

    /// Get the current series options as a JSON string.
    pub fn options(&self, chart: &ChartApi) -> Option<String> {
        let s = chart.inner.series_get_options(self.id);
        if s.is_empty() {
            None
        } else {
            Some(s)
        }
    }

    // -- Visibility --

    /// Set series visibility.
    pub fn set_visible(&self, chart: &mut ChartApi, visible: bool) {
        if let Some(series) = chart.inner.state.series.get_mut(self.id) {
            series.visible = visible;
            chart.invalidate();
        }
    }

    /// Get series visibility.
    pub fn visible(&self, chart: &ChartApi) -> bool {
        chart
            .inner
            .state
            .series
            .get(self.id)
            .map(|s| s.visible)
            .unwrap_or(false)
    }

    // -- Pane (v5) --

    /// Get the pane this series belongs to (v5: `ISeriesApi.getPane()`).
    pub fn get_pane(&self, chart: &ChartApi) -> Option<PaneApi> {
        chart.inner.state.series.get(self.id).map(|s| PaneApi {
            index: s.pane_index as u32,
        })
    }

    /// Move this series to a different pane.
    pub fn move_to_pane(&self, chart: &mut ChartApi, pane: &PaneApi) -> bool {
        let ok = chart.inner.state.move_series_to_pane(self.id, pane.index);
        if ok {
            chart.invalidate();
        }
        ok
    }

    /// Get the z-order of this series within its pane (v5: `seriesOrder()`).
    pub fn series_order(&self, chart: &ChartApi) -> Option<u32> {
        let series = chart.inner.state.series.get(self.id)?;
        let pane_idx = series.pane_index;
        let mut order = 0u32;
        for s in chart.inner.state.series.series.iter() {
            if s.pane_index == pane_idx {
                if s.id == self.id {
                    return Some(order);
                }
                order += 1;
            }
        }
        None
    }

    /// Set the z-order of this series within its pane.
    pub fn set_series_order(&self, chart: &mut ChartApi, order: u32) -> bool {
        chart.inner.series_set_order(self.id, order)
    }

    // -- Price Lines --

    /// Create a price line on this series. Returns the price line ID.
    pub fn create_price_line(&self, chart: &mut ChartApi, options: PriceLineOptions) -> u32 {
        if let Some(series) = chart.inner.state.series.get_mut(self.id) {
            let id = series.add_price_line(options);
            chart.invalidate();
            id
        } else {
            u32::MAX
        }
    }

    /// Remove a price line by ID.
    pub fn remove_price_line(&self, chart: &mut ChartApi, line_id: u32) -> bool {
        if let Some(series) = chart.inner.state.series.get_mut(self.id) {
            let ok = series.remove_price_line(line_id);
            if ok {
                chart.invalidate();
            }
            ok
        } else {
            false
        }
    }

    // -- Markers --

    /// Set markers on this series from a JSON array.
    pub fn set_markers(&self, chart: &mut ChartApi, markers_json: &str) -> bool {
        chart.inner.series_set_markers(self.id, markers_json)
    }

    /// Get markers as a JSON string.
    pub fn markers(&self, chart: &ChartApi) -> Option<String> {
        let s = chart.inner.series_markers();
        if s.is_empty() {
            None
        } else {
            Some(s)
        }
    }

    /// Number of bars in a logical index range.
    pub fn bars_in_logical_range(&self, chart: &ChartApi, from: f32, to: f32) -> u32 {
        chart.inner.series_bars_in_logical_range(self.id, from, to)
    }
}

// ════════════════════════════════════════════════════════════════════════════
//  v5: TimeScaleApi
// ════════════════════════════════════════════════════════════════════════════

/// Provides v5 `ITimeScaleApi` methods. Borrows [`ChartApi`].
pub struct TimeScaleApi<'a> {
    chart: &'a mut ChartApi,
}

impl<'a> TimeScaleApi<'a> {
    /// Scroll to a specific bar position (fractional index from the right).
    pub fn scroll_to_position(&mut self, position: f32) {
        let _ = self.chart.inner.time_scale_scroll_to_position(position);
        self.chart.invalidate();
    }

    /// Scroll so the last bar is visible (right edge).
    pub fn scroll_to_real_time(&mut self) {
        let _ = self.chart.inner.time_scale_scroll_to_real_time();
        self.chart.invalidate();
    }

    /// Get the visible time range as unix timestamps.
    pub fn get_visible_range(&self) -> Option<(i64, i64)> {
        self.chart.inner.time_scale_visible_range()
    }

    /// Set the visible time range by start/end timestamps.
    pub fn set_visible_range(&mut self, start: i64, end: i64) {
        let _ = self.chart.inner.time_scale_set_visible_range(start, end);
        self.chart.invalidate();
    }

    /// Get the visible logical range (bar indices).
    pub fn get_visible_logical_range(&self) -> (f64, f64) {
        self.chart.inner.time_scale_visible_logical_range()
    }

    /// Set the visible logical range by bar indices.
    pub fn set_visible_logical_range(&mut self, first: f64, last: f64) {
        let _ = self
            .chart
            .inner
            .time_scale_set_visible_logical_range(first, last);
        self.chart.invalidate();
    }

    /// Reset time scale to default (fit content).
    pub fn reset(&mut self) {
        let _ = self.chart.inner.time_scale_reset();
        self.chart.invalidate();
    }

    /// Get the time scale width in logical pixels.
    pub fn width(&self) -> f32 {
        self.chart.inner.time_scale_width()
    }

    /// Get the time scale height in logical pixels.
    pub fn height(&self) -> f32 {
        self.chart.inner.time_scale_height()
    }

    /// Apply options via JSON.
    pub fn apply_options(&mut self, json: &str) -> bool {
        let ok = self.chart.inner.time_scale_apply_options(json);
        if ok {
            self.chart.invalidate();
        }
        ok
    }
}

// ════════════════════════════════════════════════════════════════════════════
//  v5: PriceScaleApi
// ════════════════════════════════════════════════════════════════════════════

/// Provides v5 `IPriceScaleApi` methods. Borrows [`ChartApi`].
pub struct PriceScaleApi<'a> {
    chart: &'a mut ChartApi,
    pane_index: u32,
}

impl<'a> PriceScaleApi<'a> {
    /// Get the price scale mode.
    pub fn mode(&self) -> PriceScaleModeKind {
        self.chart.inner.price_scale_get_mode(self.pane_index)
    }

    /// Set the price scale mode.
    pub fn set_mode(&mut self, mode: PriceScaleModeKind) {
        let _ = self.chart.inner.price_scale_set_mode(self.pane_index, mode);
        self.chart.invalidate();
    }

    /// Get the current visible price range.
    pub fn range(&self) -> Option<(f64, f64)> {
        self.chart.inner.price_scale_get_range(self.pane_index)
    }

    /// Get the price scale width in pixels.
    pub fn width(&self) -> f32 {
        self.chart.inner.price_scale_width(self.pane_index)
    }

    /// Apply options via JSON.
    pub fn apply_options(&mut self, json: &str) -> bool {
        let ok = self.chart.inner.price_scale_apply_options(json);
        if ok {
            self.chart.invalidate();
        }
        ok
    }

    /// Get the current options as a JSON string.
    pub fn options(&self) -> String {
        self.chart.inner.price_scale_get_options(self.pane_index)
    }
}

// ════════════════════════════════════════════════════════════════════════════
//  v5: ChartApi — the main entry point
// ════════════════════════════════════════════════════════════════════════════

/// Safe, idiomatic wrapper around [`Chart`].
///
/// Provides the v5 `IChartApi` interface: unified `add_series`, pane
/// management, coordinate translation, and sub-API accessors.
///
/// `ChartApi` takes ownership of the `Chart`. Drop it to release all
/// resources.
pub struct ChartApi {
    pub(crate) inner: Chart,
}

impl ChartApi {
    /// Wrap an existing `Chart` in the v5 SDK.
    pub fn new(chart: Chart) -> Self {
        Self { inner: chart }
    }

    /// Create a chart with the given viewport size.
    pub fn with_size(width: u32, height: u32, scale_factor: f64) -> Self {
        Self {
            inner: Chart::new(width, height, scale_factor),
        }
    }

    /// Mark the chart as needing a redraw (used internally after state
    /// mutations).
    fn invalidate(&mut self) {
        self.inner
            .state
            .pending_mask
            .set_global(InvalidationLevel::Full);
    }

    // -- Rendering --

    /// Render the chart unconditionally.
    pub fn render(&mut self) {
        self.inner.render();
    }

    /// Render only if the invalidation mask says a redraw is needed.
    pub fn render_if_needed(&mut self) -> bool {
        self.inner.render_if_needed()
    }

    /// Resize the chart viewport.
    pub fn resize(&mut self, width: u32, height: u32, scale_factor: f64) {
        self.inner.resize(width, height, scale_factor);
    }

    /// Fit all data into the visible viewport.
    pub fn fit_content(&mut self) {
        self.inner.fit_content();
    }

    // -- Input --

    /// Handle a pointer/mouse move. Returns true if a redraw is needed.
    pub fn pointer_move(&mut self, x: f32, y: f32) -> bool {
        self.inner.pointer_move(x, y)
    }

    /// Handle a pointer/mouse button press. Returns true if a redraw is needed.
    pub fn pointer_down(&mut self, x: f32, y: f32, button: u8) -> bool {
        self.inner.pointer_down(x, y, button)
    }

    /// Handle a pointer/mouse button release. Returns true if a redraw is needed.
    pub fn pointer_up(&mut self, x: f32, y: f32, button: u8) -> bool {
        self.inner.pointer_up(x, y, button)
    }

    /// Handle pointer leaving the chart area. Returns true if a redraw is needed.
    pub fn pointer_leave(&mut self) -> bool {
        self.inner.pointer_leave()
    }

    /// Handle a scroll/wheel event. Returns true if a redraw is needed.
    pub fn scroll(&mut self, dx: f32, dy: f32) -> bool {
        self.inner.scroll(dx, dy)
    }

    /// Handle a zoom event. Returns true if a redraw is needed.
    pub fn zoom(&mut self, factor: f32, center_x: f32) -> bool {
        self.inner.zoom(factor, center_x)
    }

    /// Handle a pinch-to-zoom gesture. Returns true if a redraw is needed.
    pub fn pinch(&mut self, scale: f32, center_x: f32, center_y: f32) -> bool {
        self.inner.pinch(scale, center_x, center_y)
    }

    /// Handle a keyboard key-down event. Returns true if a redraw is needed.
    pub fn key_down(&mut self, key_code: u32) -> bool {
        self.inner.key_down(key_code)
    }

    // -- Data (primary series) --

    /// Set primary OHLC data from a slice of bars.
    pub fn set_data(&mut self, bars: Vec<OhlcBar>) {
        self.inner.set_data(bars);
    }

    /// Set primary series rendering type.
    pub fn set_series_type(&mut self, type_index: u32) {
        self.inner.set_series_type(type_index);
    }

    /// Number of primary OHLC bars.
    pub fn bar_count(&self) -> usize {
        self.inner.bar_count()
    }

    // -- v5: Unified addSeries --

    /// Add a new series to the chart (v5 unified API).
    pub fn add_series(&mut self, definition: SeriesDefinition) -> SeriesApi {
        let series = match definition {
            SeriesDefinition::Ohlc => Series::ohlc(0, vec![]),
            SeriesDefinition::Candlestick => Series::candlestick(0, vec![]),
            SeriesDefinition::Line => Series::line(0, vec![]),
            SeriesDefinition::Area => Series::area(0, vec![]),
            SeriesDefinition::Histogram => Series::histogram(0, vec![]),
            SeriesDefinition::Baseline { base_value } => Series::baseline(0, vec![], base_value),
        };
        let id = self.inner.state.series.add(series);
        self.invalidate();
        SeriesApi { id }
    }

    /// Remove a series from the chart.
    pub fn remove_series(&mut self, series: &SeriesApi) -> bool {
        let ok = self.inner.state.series.remove(series.id);
        if ok {
            self.invalidate();
        }
        ok
    }

    /// Get the number of series.
    pub fn series_count(&self) -> usize {
        self.inner.state.series.len()
    }

    // -- v5: Pane management --

    /// Add a new pane. Returns a `PaneApi` handle. `height_stretch`
    /// controls relative height (1.0 = equal share).
    pub fn add_pane(&mut self, height_stretch: f32) -> PaneApi {
        let index = self.inner.state.add_pane(height_stretch);
        PaneApi { index }
    }

    /// Remove a pane by handle. Pane 0 (main) cannot be removed.
    pub fn remove_pane(&mut self, pane: &PaneApi) -> bool {
        self.inner.state.remove_pane(pane.index)
    }

    /// Swap two panes.
    pub fn swap_panes(&mut self, a: &PaneApi, b: &PaneApi) -> bool {
        self.inner.state.swap_panes(a.index, b.index)
    }

    /// Get the number of panes.
    pub fn pane_count(&self) -> usize {
        self.inner.state.panes.len()
    }

    /// Get the layout rect of a pane: `(x, y, width, height)`.
    pub fn pane_size(&self, pane: &PaneApi) -> Option<(f32, f32, f32, f32)> {
        self.inner.state.pane_size(pane.index)
    }

    /// Set the height stretch factor of a pane.
    pub fn set_pane_stretch(&mut self, pane: &PaneApi, stretch: f32) -> bool {
        let idx = pane.index as usize;
        if idx >= self.inner.state.panes.len() {
            return false;
        }
        let clamped = stretch.clamp(MIN_PANE_STRETCH, MAX_PANE_STRETCH);
        self.inner.state.panes[idx].height_stretch = clamped;
        self.inner.state.update_panes_layout();
        self.invalidate();
        true
    }

    /// Y coordinate of the separator line *below* the given pane.
    pub fn pane_separator_y(&self, pane: &PaneApi) -> Option<f32> {
        let (x, y, _w, h) = self.pane_size(pane)?;
        let _ = x;
        Some(y + h)
    }

    /// Get the height fraction (0..=1) of a pane relative to all panes.
    pub fn pane_height_fraction(&self, pane: &PaneApi) -> f32 {
        let idx = pane.index as usize;
        self.inner.state.pane_height_fraction(idx)
    }

    /// Total plot-area height in CSS pixels.
    pub fn plot_area_height(&self) -> f32 {
        self.inner.state.layout.plot_area.height
    }

    /// Set the height fraction of a pane (0..=1). The other panes share the
    /// remaining height proportionally to their current fractions.
    ///
    /// Returns `true` if the layout changed.
    pub fn set_pane_height_fraction(&mut self, pane: &PaneApi, fraction: f32) -> bool {
        let idx = pane.index as usize;
        if self.inner.state.set_pane_height_fraction(idx, fraction) {
            self.invalidate();
            true
        } else {
            false
        }
    }

    // -- Options --

    /// Apply chart options from a JSON string.
    pub fn apply_options(&mut self, json: &str) -> bool {
        let ok = self.inner.apply_options(json);
        if ok {
            self.invalidate();
        }
        ok
    }

    /// Get current chart options as a JSON string.
    pub fn options(&self) -> String {
        self.inner.get_options()
    }

    // -- Coordinate translation --

    /// Convert a price to a Y pixel coordinate (uses pane 0).
    pub fn price_to_coordinate(&self, price: f64) -> f32 {
        self.inner.price_to_coordinate(0, price)
    }

    /// Convert a Y pixel coordinate to a price (uses pane 0).
    pub fn coordinate_to_price(&self, y: f32) -> f64 {
        self.inner.coordinate_to_price(0, y)
    }

    /// Convert a logical index to an X pixel coordinate.
    pub fn logical_to_coordinate(&self, logical: f64) -> f32 {
        self.inner.logical_to_coordinate(logical)
    }

    /// Convert an X pixel coordinate to a logical index.
    pub fn coordinate_to_logical(&self, x: f32) -> f64 {
        self.inner.coordinate_to_logical(x)
    }

    // -- Sub-API accessors --

    /// Get the time scale API for this chart.
    pub fn time_scale(&mut self) -> TimeScaleApi<'_> {
        TimeScaleApi { chart: self }
    }

    /// Get the price scale API for a specific pane (default: pane 0).
    pub fn price_scale(&mut self, pane_index: u32) -> PriceScaleApi<'_> {
        PriceScaleApi {
            chart: self,
            pane_index,
        }
    }

    // -- Crosshair --

    /// Programmatically set the crosshair position.
    pub fn set_crosshair_position(&mut self, price: f64, time: i64, series: &SeriesApi) -> bool {
        self.inner.set_crosshair_position(price, time, series.id)
    }

    /// Clear the crosshair position.
    pub fn clear_crosshair_position(&mut self) -> bool {
        self.inner.clear_crosshair_position()
    }

    // -- Formatting helpers --

    /// Format a price using the chart's localization settings.
    pub fn format_price(&self, price: f64) -> String {
        self.inner.format_price(price)
    }

    /// Format a timestamp as a date string.
    pub fn format_date(&self, timestamp: i64) -> String {
        self.inner.format_date(timestamp)
    }

    /// Format a timestamp as a time string.
    pub fn format_time(&self, timestamp: i64) -> String {
        self.inner.format_time(timestamp)
    }

    /// Direct access to the inner [`Chart`] (for renderer access, etc.).
    pub fn chart(&self) -> &Chart {
        &self.inner
    }

    /// Mutable access to the inner [`Chart`].
    pub fn chart_mut(&mut self) -> &mut Chart {
        &mut self.inner
    }
}
