//! Lumen Charts — Rust Demo (Iced 0.14 backend).
//!
//! Cross-platform desktop demo using Iced 0.14 (wgpu backend).
//! Showcases chart type switching, overlay, and MACD indicator.
//!
//! ## Architecture
//!
//! The demo is now a thin layer over the SDK's drop-in Iced widget:
//!
//! - [`IcedChart::new`] wraps a `ChartApi` and provides an
//!   `iced::widget::canvas::Canvas` widget that handles resize, pointer
//!   events, and paint automatically. There is no `Program<…>` impl
//!   boilerplate in the demo.
//! - Mutating the chart from `update` handlers goes through
//!   `IcedChart::with_chart_mut(|c| { ... })`, which closes over the
//!   shared `Rc<RefCell<ChartApi>>` inside the widget.
//!
//! All the boilerplate that used to live in `chart_widget.rs` and
//! `renderer.rs` (a custom `Program` impl, a `SharedRenderer` adapter
//! that bridged `Rc<RefCell<…>>` to `Box<dyn Renderer>`, and explicit
//! frame painting) has moved into `lumen-charts-sdk`'s
//! `renderers::iced` module.

use iced::widget::{button, column, text, Column, Row, Space};
use iced::{Element, Length, Task};

use lumen_charts_sdk::renderers::iced::{ChartWithSeparators, SeparatorMessage};
use lumen_charts_sdk::sample_data::sample_data;
use lumen_charts_sdk::{
    ChartApi, Color, HistogramDataPoint, LineDataPoint, OhlcBar, PaneApi, SeriesApi,
    SeriesDefinition,
};

// ─── Series type labels ────────────────────────────────────────────────────
const SERIES_TYPES: [&str; 6] = ["OHLC", "Candle", "Line", "Area", "Hist", "Baseline"];

// ─── MACD helpers ──────────────────────────────────────────────────────────
fn ema(values: &[f64], period: usize) -> Vec<f64> {
    if values.len() < period {
        return values.to_vec();
    }
    let k = 2.0 / (period as f64 + 1.0);
    let mut result = vec![0.0f64; values.len()];
    let sma: f64 = values[..period].iter().sum::<f64>() / period as f64;
    result[period - 1] = sma;
    for i in period..values.len() {
        result[i] = values[i] * k + result[i - 1] * (1.0 - k);
    }
    result
}

struct MacdData {
    macd_line: Vec<LineDataPoint>,
    signal_line: Vec<LineDataPoint>,
    histogram: Vec<HistogramDataPoint>,
}

fn calculate_macd(bars: &[OhlcBar]) -> MacdData {
    let closes: Vec<f64> = bars.iter().map(|b| b.close).collect();
    let ema12 = ema(&closes, 12);
    let ema26 = ema(&closes, 26);

    let start_idx = 25;
    let mut macd_values = Vec::new();
    let mut macd_times = Vec::new();
    for i in start_idx..bars.len() {
        macd_values.push(ema12[i] - ema26[i]);
        macd_times.push(bars[i].time);
    }

    let signal_values = ema(&macd_values, 9);
    let signal_start = 8;

    let mut macd_line = Vec::new();
    let mut signal_line = Vec::new();
    let mut histogram = Vec::new();

    for i in signal_start..macd_values.len() {
        let time = macd_times[i];
        let macd = macd_values[i];
        let signal = signal_values[i];
        let hist = macd - signal;

        macd_line.push(LineDataPoint { time, value: macd });
        signal_line.push(LineDataPoint {
            time,
            value: signal,
        });

        let color = if hist >= 0.0 {
            Color([0.16, 0.76, 0.49, 0.8])
        } else {
            Color([0.94, 0.27, 0.27, 0.8])
        };
        histogram.push(HistogramDataPoint {
            time,
            value: hist,
            color: Some(color),
        });
    }

    MacdData {
        macd_line,
        signal_line,
        histogram,
    }
}

// ─── Application state ─────────────────────────────────────────────────────
#[derive(Debug, Clone)]
enum Message {
    SetSeriesType(usize),
    FitContent,
    ToggleOverlay,
    ToggleMacd,
    Separator(SeparatorMessage),
}

impl From<SeparatorMessage> for Message {
    fn from(sep: SeparatorMessage) -> Self {
        Message::Separator(sep)
    }
}

struct ChartApp {
    /// The SDK widget — owns the chart. Use `with_chart_mut(...)` to mutate.
    chart_view: ChartWithSeparators,
    current_series_type: usize,
    overlay_active: bool,
    overlay_series: Option<SeriesApi>,
    macd_active: bool,
    macd_pane: Option<PaneApi>,
    macd_series: Vec<SeriesApi>,
    sample_bars: Vec<OhlcBar>,
}

impl ChartApp {
    fn new() -> Self {
        env_logger::try_init().ok();

        // Start with a placeholder size — the chart core needs a non-zero
        // layout to initialize, but Iced will report the actual window size
        // on the first `IcedProgram::draw` and we'll resize there.
        let width = 800u32;
        let height = 600u32;
        let scale_factor = 1.0f64;

        // The chart is constructed directly with viewport dimensions.
        // No renderer type, no `Box`, no trait object — `IcedChart` paints
        // directly from `ChartState` when Iced calls its `Program::draw`.
        let mut chart = ChartApi::with_size(width, height, scale_factor);
        let bars = sample_data();
        chart.set_data(bars.clone());
        chart.fit_content();
        chart.render();

        Self {
            chart_view: ChartWithSeparators::new(chart),
            current_series_type: 0,
            overlay_active: false,
            overlay_series: None,
            macd_active: false,
            macd_pane: None,
            macd_series: Vec::new(),
            sample_bars: bars,
        }
    }

    fn toggle_overlay(&mut self) {
        self.chart_view.with_chart_mut(|chart| {
            if let Some(series) = self.overlay_series.take() {
                chart.remove_series(&series);
                chart.render();
                self.overlay_active = false;
            } else {
                let overlay_data: Vec<LineDataPoint> = self
                    .sample_bars
                    .iter()
                    .map(|b| LineDataPoint {
                        time: b.time,
                        value: b.close - 15.0,
                    })
                    .collect();

                let series = chart.add_series(SeriesDefinition::Area);
                series.set_line_data(chart, &overlay_data);
                chart.render();
                self.overlay_series = Some(series);
                self.overlay_active = true;
            }
        });
    }

    fn toggle_macd(&mut self) {
        self.chart_view.with_chart_mut(|chart| {
            if let Some(pane) = self.macd_pane.take() {
                for series in &self.macd_series {
                    chart.remove_series(series);
                }
                chart.remove_pane(&pane);
                self.macd_series.clear();
                chart.render();
                self.macd_active = false;
            } else {
                let macd = calculate_macd(&self.sample_bars);

                let pane = chart.add_pane(0.3);

                let hist_series = chart.add_series(SeriesDefinition::Histogram);
                hist_series.set_histogram_data(chart, &macd.histogram);
                hist_series.move_to_pane(chart, &pane);

                let macd_line_series = chart.add_series(SeriesDefinition::Line);
                macd_line_series.set_line_data(chart, &macd.macd_line);
                macd_line_series.move_to_pane(chart, &pane);
                macd_line_series
                    .apply_options(chart, r#"{"color":[0.2,0.6,1.0,1.0],"lineWidth":1.5}"#);

                let signal_series = chart.add_series(SeriesDefinition::Line);
                signal_series.set_line_data(chart, &macd.signal_line);
                signal_series.move_to_pane(chart, &pane);
                signal_series
                    .apply_options(chart, r#"{"color":[1.0,0.6,0.2,1.0],"lineWidth":1.5}"#);

                self.macd_pane = Some(pane);
                self.macd_series = vec![hist_series, macd_line_series, signal_series];
                chart.render();
                self.macd_active = true;
            }
        });
    }

    fn set_series_type(&mut self, type_idx: usize) {
        self.current_series_type = type_idx;
        self.chart_view.with_chart_mut(|chart| {
            chart.set_series_type(type_idx as u32);
            chart.render();
        });
    }

    fn fit_content(&mut self) {
        self.chart_view.with_chart_mut(|chart| {
            chart.fit_content();
            chart.render();
        });
    }
}

// ─── Iced 0.14 functional application API ─────────────────────────────────
fn main() -> iced::Result {
    iced::application(ChartApp::new, update, view)
        .title("Lumen Charts — Rust Demo")
        .theme(iced::Theme::Dark)
        .run()
}

fn update(state: &mut ChartApp, message: Message) -> Task<Message> {
    match message {
        Message::SetSeriesType(i) => state.set_series_type(i),
        Message::FitContent => state.fit_content(),
        Message::ToggleOverlay => state.toggle_overlay(),
        Message::ToggleMacd => state.toggle_macd(),
        Message::Separator(SeparatorMessage::Drag {
            pane_index,
            pixel_height,
        }) => {
            let total = state
                .chart_view
                .chart_handle()
                .borrow()
                .plot_area_height()
                .max(1.0);
            let frac = (pixel_height / total).clamp(0.05, 0.95);
            let pane = PaneApi::from_index(pane_index as u32);
            state
                .chart_view
                .chart_handle()
                .borrow_mut()
                .set_pane_height_fraction(&pane, frac);
            let _ = pane_index;
        }
    }
    Task::none()
}

fn view(state: &ChartApp) -> Column<'_, Message, iced::Theme, iced::Renderer> {
    // Toolbar: chart-type buttons
    let type_buttons =
        SERIES_TYPES
            .iter()
            .enumerate()
            .fold(Row::new().spacing(4), |row, (i, name)| {
                row.push(
                    button(text(*name))
                        .on_press(Message::SetSeriesType(i))
                        .style(if i == state.current_series_type {
                            iced::widget::button::primary
                        } else {
                            iced::widget::button::secondary
                        }),
                )
            });

    let toolbar: Row<'_, Message, _, _> = Row::new()
        .padding(8)
        .spacing(8)
        .align_y(iced::Alignment::Center)
        .push(text("Chart:"))
        .push(type_buttons)
        .push(Space::new().width(Length::Fixed(16.0)))
        .push(
            button(text("Fit"))
                .on_press(Message::FitContent)
                .style(iced::widget::button::secondary),
        )
        .push(
            button(text(if state.overlay_active {
                "Overlay ✓"
            } else {
                "Overlay"
            }))
            .on_press(Message::ToggleOverlay)
            .style(if state.overlay_active {
                iced::widget::button::primary
            } else {
                iced::widget::button::secondary
            }),
        )
        .push(
            button(text(if state.macd_active {
                "MACD ✓"
            } else {
                "MACD"
            }))
            .on_press(Message::ToggleMacd)
            .style(if state.macd_active {
                iced::widget::button::primary
            } else {
                iced::widget::button::secondary
            }),
        )
        .push(Space::new().width(Length::Fill))
        .push(text(format!(
            "{} • {} bars",
            SERIES_TYPES[state.current_series_type],
            state.sample_bars.len()
        )));

    // Build the chart widget. The widget owns the chart internally, so
    // we can construct it here and pass it straight into the column.
    let chart_widget: Element<'_, Message, iced::Theme, iced::Renderer> =
        state.chart_view.clone().view(Message::Separator).into();

    column![toolbar, chart_widget]
}
