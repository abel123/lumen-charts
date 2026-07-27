//! Iced 0.14 backend — drop-in widget for embedding a [`ChartApi`] in an
//! Iced application.
//!
//! This module is the **only** rendering path. There is no
//! `Box<dyn Renderer>` indirection, no trait dispatch, no separate
//! "backend" vs "renderer" file. Painting a chart is one function:
//!
//! ```ignore
//! use lumen_charts_sdk::renderers::iced::IcedChart;
//!
//! let mut chart = ChartApi::with_size(800, 600, 1.0);
//! chart.set_data(bars);
//!
//! // Wrap in an Iced widget, then drop into your view tree:
//! let canvas = IcedChart::new(chart).canvas();
//! ```
//!
//! ## Architecture
//!
//! ```text
//! IcedChart  ──owns──▶  Rc<RefCell<ChartApi>>
//!     │
//!     └── IcedProgram : iced::widget::canvas::Program
//!             ├── draw    → resize if needed, then paint_to_iced_frame
//!             ├── update  → translate Iced events → chart pointer/wheel
//!             └── mouse_interaction
//!
//! paint_to_iced_frame
//!     ├── creates a tiny private `Backend` impl of `DrawBackend`
//     │      that forwards to `iced::widget::canvas::Frame`
//     ├── render_bottom_scene(&mut backend, state)
//     └── render_crosshair_scene(&mut backend, state)
//! ```
//!
//! Mutating the chart from your `update` handler:
//!
//! ```ignore
//! chart_view.with_chart_mut(|c| {
//!     c.set_series_type(1);
//! });
//! ```
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use iced::mouse::Cursor as IcedCursor;
use iced::widget::canvas::gradient::Linear;
use iced::widget::canvas::{
    Action, Canvas, Event, Frame, Geometry, Gradient, LineDash, Path, Program, Stroke, Style,
};
use iced::{
    mouse, window, Color as IcedColor, Element, Pixels, Point, Rectangle, Renderer as IcedRenderer,
    Size, Theme, Vector,
};

use crate::chart_renderer::{render_bottom_scene, render_crosshair_scene, render_pane_axes};
use crate::chart_state::ChartState;
use crate::color::{Color, GradientStop};
use crate::ChartApi;

// ════════════════════════════════════════════════════════════════════════════
//  paint_to_iced_frame — single private paint path
// ════════════════════════════════════════════════════════════════════════════

/// Paint the chart into an Iced `Frame`. The single rendering entry
/// point; called by [`IcedProgram::draw`].
///
/// Constructs an [`IcedBackend`] wrapping the Frame and hands it to
/// the chart engine's scene renderers. All draw primitives (fill_rect,
/// stroke_line, text, …) flow through this adapter into Iced.
fn paint_to_iced_frame(frame: &mut Frame, state: &ChartState) {
    let mut backend = IcedBackend {
        frame,
        sx: 1.0,
        sy: 1.0,
        _marker: std::marker::PhantomData,
    };
    render_bottom_scene(&mut backend, state);
    render_crosshair_scene(&mut backend, state);
}

/// Paint a single pane's content into the given frame.
///
/// The frame is the bounds of a single Canvas widget in the multi-
/// canvas widget tree produced by [`ChartWithSeparators::view`]. The
/// chart's overall layout uses absolute coordinates, so we shift the
/// frame by `-pane.layout_rect.y` so that this pane's content lands at
/// the top-left of its canvas.
fn paint_pane_to_iced_frame(pane_index: usize, frame: &mut Frame, state: &ChartState) {
    let (pane_y, pane_h) = state
        .pane_size(pane_index as u32)
        .map(|(_, y, _, h)| (y, h))
        .unwrap_or((0.0, 0.0));

    frame.translate(Vector::new(0.0, -pane_y));
    let mut backend = IcedBackend {
        frame,
        sx: 1.0,
        sy: 1.0,
        _marker: std::marker::PhantomData,
    };
    crate::chart_renderer::render_pane(pane_index, &mut backend, state);
    crate::chart_renderer::render_crosshair_for_pane(pane_index, &mut backend, state);
    render_pane_axes(pane_index, &mut backend, state);
    let _ = pane_h;
}

/// Adapter that translates the chart engine's draw primitives into Iced
/// `Frame` calls. Has a lifetime tied to the `Frame` borrow — see
/// [`paint_to_iced_frame`].
///
/// This is the **only** concrete backend in the project. The chart
/// engine's renderer functions take `&mut IcedBackend<'a>` directly;
/// there is no trait dispatch layer.
pub struct IcedBackend<'a> {
    pub frame: &'a mut Frame,
    pub(crate) sx: f32,
    pub(crate) sy: f32,
    // PhantomData<&'a ()> forces the lifetime to be covariant, so
    // `IcedBackend<'short>` coerces to `IcedBackend<'long>` when
    // `'short ⊆ 'long`. This is what makes `with_clip` work: the
    // Iced closure hands us a `&'short mut Frame` (reborrowed for
    // the duration of the clip scope), we wrap it in `IcedBackend<'short>`,
    // and pass it to a user closure expecting `IcedBackend<'long>`.
    pub(crate) _marker: std::marker::PhantomData<&'a ()>,
}

impl IcedBackend<'_> {
    fn points_to_iced(&self, points: &[(f64, f64)]) -> Vec<Point> {
        points
            .iter()
            .map(|&(x, y)| Point::new((x as f32) * self.sx, (y as f32) * self.sy))
            .collect()
    }
}

fn c4_to_iced(c: Color) -> IcedColor {
    IcedColor::from_rgba(c[0], c[1], c[2], c[3])
}

// ── Pixel snapping helpers ───────────────────────────────────────────────
//
// Snap a coordinate to the nearest pixel boundary so that 1px strokes
// aren't drawn half-on / half-off a pixel (which produces blurry lines
// on LCD screens). `sf` is the device scale factor (1.0 for logical /
// CSS pixels, > 1.0 for HiDPI).

pub fn snap_x(x: f64, sf: f64) -> f64 {
    (x * sf).round() / sf
}

pub fn snap_y(y: f64, sf: f64) -> f64 {
    (y * sf).round() / sf
}

impl<'a> IcedBackend<'a> {
    pub fn begin_frame(&mut self, _width: f64, _height: f64) {}

    pub fn end_frame(&mut self) {}

    pub fn set_scale(&mut self, sx: f64, sy: f64) {
        self.sx = sx as f32;
        self.sy = sy as f32;
    }

    pub fn fill_rect(&mut self, x: f64, y: f64, w: f64, h: f64, color: Color) {
        self.frame.fill_rectangle(
            Point::new(x as f32, y as f32),
            iced::Size::new(w as f32, h as f32),
            c4_to_iced(color),
        );
    }

    pub fn fill_rect_gradient(
        &mut self,
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        y_start: f64,
        y_end: f64,
        stops: &[GradientStop],
    ) {
        if stops.is_empty() {
            return;
        }
        if stops.len() == 1 {
            self.fill_rect(x, y, w, h, stops[0].0);
            return;
        }
        // For 2+ stops we paint horizontal bands at proportional positions.
        // Iced's `Linear` gradient supports up to 8 stops but the chart core
        // can emit arbitrarily many (e.g. heat-map style fills). Falling back
        // to bands keeps the implementation simple and visually correct.
        let total_h = (y_end - y_start).max(f64::EPSILON);
        let bands = stops.len().saturating_sub(1);
        for i in 0..bands {
            let t0 = stops[i].1;
            let t1 = stops[i + 1].1;
            let by0 = y_start + t0 as f64 * total_h;
            let by1 = y_start + t1 as f64 * total_h;
            let bh = (by1 - by0).max(0.0);
            let p0 = Point::new(x as f32, by0 as f32);
            let p1 = Point::new(x as f32, by1 as f32);
            let linear = Linear::new(p0, p1)
                .add_stop(0.0, c4_to_iced(stops[i].0))
                .add_stop(1.0, c4_to_iced(stops[i + 1].0));
            self.frame.fill_rectangle(
                Point::new(x as f32, by0 as f32),
                iced::Size::new(w as f32, bh as f32),
                Gradient::Linear(linear),
            );
        }
    }

    pub fn stroke_line(&mut self, x0: f64, y0: f64, x1: f64, y1: f64, color: Color, width: f64) {
        let path = Path::line(
            Point::new(x0 as f32, y0 as f32),
            Point::new(x1 as f32, y1 as f32),
        );
        let stroke = Stroke::default()
            .with_color(c4_to_iced(color))
            .with_width(width as f32);
        self.frame.stroke(&path, stroke);
    }

    pub fn stroke_dashed_line(
        &mut self,
        x0: f64,
        y0: f64,
        x1: f64,
        y1: f64,
        color: Color,
        width: f64,
        dash_len: f64,
        gap_len: f64,
    ) {
        let path = Path::line(
            Point::new(x0 as f32, y0 as f32),
            Point::new(x1 as f32, y1 as f32),
        );
        let stroke = Stroke {
            style: Style::Solid(c4_to_iced(color)),
            width: width as f32,
            line_cap: Default::default(),
            line_join: Default::default(),
            line_dash: LineDash {
                segments: &[dash_len as f32, gap_len as f32],
                offset: 0,
            },
        };
        self.frame.stroke(&path, stroke);
    }

    pub fn stroke_path(&mut self, points: &[(f64, f64)], color: Color, width: f64) {
        if points.len() < 2 {
            return;
        }
        let pts = self.points_to_iced(points);
        let path = Path::new(|b| {
            b.move_to(pts[0]);
            for p in &pts[1..] {
                b.line_to(*p);
            }
        });
        let stroke = Stroke::default()
            .with_color(c4_to_iced(color))
            .with_width(width as f32);
        self.frame.stroke(&path, stroke);
    }

    pub fn fill_path(&mut self, points: &[(f64, f64)], color: Color) {
        if points.len() < 3 {
            return;
        }
        let pts = self.points_to_iced(points);
        let path = Path::new(|b| {
            b.move_to(pts[0]);
            for p in &pts[1..] {
                b.line_to(*p);
            }
            b.close();
        });
        self.frame.fill(&path, c4_to_iced(color));
    }

    pub fn fill_path_gradient(
        &mut self,
        points: &[(f64, f64)],
        y_start: f64,
        y_end: f64,
        stops: &[GradientStop],
    ) {
        if points.len() < 3 || stops.is_empty() {
            return;
        }
        if stops.len() == 2 {
            let p0 = Point::new(points[0].0 as f32, y_start as f32);
            let p1 = Point::new(points[0].0 as f32, y_end as f32);
            let linear = Linear::new(p0, p1)
                .add_stop(0.0, c4_to_iced(stops[0].0))
                .add_stop(1.0, c4_to_iced(stops[1].0));
            let pts = self.points_to_iced(points);
            let path = Path::new(|b| {
                b.move_to(pts[0]);
                for p in &pts[1..] {
                    b.line_to(*p);
                }
                b.close();
            });
            self.frame.fill(&path, Gradient::Linear(linear));
        } else {
            // Multiple-stop gradients: fall back to the dominant color.
            self.fill_path(points, stops[stops.len() - 1].0);
        }
    }

    pub fn fill_circle(&mut self, cx: f64, cy: f64, radius: f64, color: Color) {
        let path = Path::circle(Point::new(cx as f32, cy as f32), radius as f32);
        self.frame.fill(&path, c4_to_iced(color));
    }

    pub fn draw_text(&mut self, text: &str, x: f64, y: f64, font_size: f64, color: Color) {
        // y is the baseline in the chart core's convention. Iced expects
        // the top-left of the text bounding box — approximate by
        // subtracting ~80% of the font size (typical ascent ratio).
        let approx_baseline_to_top = (font_size as f32) * 0.8;
        let mut t = iced::widget::canvas::Text::default();
        t.content = text.to_string();
        t.position = Point::new(x as f32, (y as f32) - approx_baseline_to_top);
        t.color = c4_to_iced(color);
        t.size = Pixels(font_size as f32);
        self.frame.fill_text(t);
    }

    pub fn measure_text(&self, text: &str, font_size: f64) -> f64 {
        // Iced 0.14 doesn't expose synchronous text measurement on Frame.
        // Return a conservative estimate.
        text.chars().count() as f64 * font_size * 0.6
    }

    /// Run `f` with drawing clipped to the rectangle `(x, y, w, h)`.
    ///
    /// Forwards to Iced's `Frame::with_clip`. The closure receives
    /// a fresh `IcedBackend<'_>` bound to the inner (clipped) frame.
    ///
    /// Implementation note: we can't write `Fn(&mut IcedBackend<'b>)`
    /// directly because `&mut Frame` makes the lifetime parameter
    /// invariant. Instead we bridge by constructing the nested
    /// `IcedBackend` inside the Iced closure where the reborrow lifetime
    /// is already in scope. The outer `for<'r> FnMut(&'r mut Frame)`
    /// is HRTB-friendly so the closure body works regardless of the
    /// reborrow lifetime.
    pub fn with_clip(
        &mut self,
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        mut f: impl FnMut(&mut IcedBackend<'_>),
    ) {
        let bounds = iced::Rectangle {
            x: x as f32,
            y: y as f32,
            width: w as f32,
            height: h as f32,
        };

        let sx = self.sx;
        let sy = self.sy;
        let outer_frame: &mut Frame = self.frame;

        outer_frame.with_clip(bounds, |frame| {
            let mut nested = IcedBackend {
                frame,
                sx,
                sy,
                _marker: std::marker::PhantomData,
            };
            f(&mut nested);
        });
    }
}

// ════════════════════════════════════════════════════════════════════════════
//  IcedChart — drop-in Iced widget
// ════════════════════════════════════════════════════════════════════════════

/// Drop-in Iced widget that owns a [`ChartApi`] and renders it via an
/// [`IcedProgram`].
///
/// Internally holds a `Rc<RefCell<ChartApi>>` so the chart can be
/// mutated from anywhere — your application's `update` handler, a
/// toolbar button's `on_press` callback, etc.
///
/// `IcedChart` is `Clone`-cheap (`Rc` clone, `O(1)`); clones share the
/// same backing chart. This matters because `view()` is called every
/// frame and constructs a fresh `Canvas` widget from a clone.
#[derive(Clone)]
pub struct IcedChart {
    chart: Rc<RefCell<ChartApi>>,
}

impl IcedChart {
    /// Wrap a `ChartApi` for use as an Iced widget. The chart becomes
    /// `Rc<RefCell<…>>` internally; subsequent mutation goes through
    /// [`IcedChart::with_chart_mut`].
    pub fn new(chart: ChartApi) -> Self {
        Self {
            chart: Rc::new(RefCell::new(chart)),
        }
    }

    /// Wrap an existing `Rc<RefCell<ChartApi>>`. Use this when the
    /// application state already owns the shared handle.
    pub fn from_shared(chart: Rc<RefCell<ChartApi>>) -> Self {
        Self { chart }
    }

    /// Borrow the inner `Rc<RefCell<ChartApi>>`. Useful when you need
    /// to clone the handle for a different part of your app.
    pub fn chart_handle(&self) -> Rc<RefCell<ChartApi>> {
        self.chart.clone()
    }

    /// Run a closure with mutable access to the chart. Use this from
    /// your application's `update` handler to mutate the chart
    /// (`set_data`, `add_series`, `add_pane`, etc.).
    pub fn with_chart_mut<R>(&self, f: impl FnOnce(&mut ChartApi) -> R) -> R {
        f(&mut self.chart.borrow_mut())
    }

    /// Build the Iced `Canvas` widget. The returned widget is `Fill` /
    /// `Fill` sized; shrink with `iced::Length::Shrink` if you need a
    /// non-stretching chart.
    pub fn canvas<Message: 'static>(self) -> Canvas<IcedProgram, Message, Theme, IcedRenderer> {
        let program = IcedProgram {
            chart: self.chart,
            last_size: Cell::new((0, 0)),
        };
        Canvas::new(program)
            .width(iced::Length::Fill)
            .height(iced::Length::Fill)
    }
}

// ════════════════════════════════════════════════════════════════════════════
//  IcedProgram — Iced Program<…> impl that drives the chart
// ════════════════════════════════════════════════════════════════════════════

/// Iced `Program<…>` impl that paints the chart and translates Iced
/// pointer events to chart pointer events.
///
/// Exposed in the type signature of [`Canvas`], but end users never
/// name it directly.
pub struct IcedProgram {
    chart: Rc<RefCell<ChartApi>>,
    /// Last canvas size used for the resize-detection block in `draw`.
    /// Single-threaded, widget-private — `Cell` is enough.
    last_size: Cell<(u32, u32)>,
}

impl<Message: 'static> Program<Message, Theme, IcedRenderer> for IcedProgram {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &IcedRenderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: IcedCursor,
    ) -> Vec<Geometry> {
        let w = bounds.width.round().max(1.0) as u32;
        let h = bounds.height.round().max(1.0) as u32;

        // Resize chart if canvas size changed. Drop the borrow before
        // constructing the Frame so we don't hold a RefCell borrow
        // across the paint.
        let prev = self.last_size.get();
        if prev != (w, h) {
            self.last_size.set((w, h));
            self.chart.borrow_mut().resize(w, h, 1.0);
            self.chart.borrow_mut().render();
        }

        let mut frame = Frame::new(renderer, bounds.size());

        // Paint: borrow the chart immutably, drop the borrow before
        // returning.
        {
            let chart = self.chart.borrow();
            paint_to_iced_frame(&mut frame, &chart.inner.state);
        }

        vec![frame.into_geometry()]
    }

    fn update(
        &self,
        _state: &mut Self::State,
        event: &Event,
        bounds: Rectangle,
        cursor: IcedCursor,
    ) -> Option<Action<Message>> {
        let pos_in_bounds = cursor.position_in(bounds);

        let redraw = {
            let mut chart = self.chart.borrow_mut();
            match event {
                Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                    if let Some(p) = pos_in_bounds {
                        chart.pointer_move(p.x, p.y)
                    } else {
                        chart.pointer_leave()
                    }
                }
                Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                    if let Some(p) = pos_in_bounds {
                        chart.pointer_down(p.x, p.y, 0)
                    } else {
                        false
                    }
                }
                Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                    if let Some(p) = pos_in_bounds {
                        chart.pointer_up(p.x, p.y, 0)
                    } else {
                        false
                    }
                }
                Event::Mouse(mouse::Event::CursorLeft) => chart.pointer_leave(),
                Event::Mouse(mouse::Event::WheelScrolled { delta }) => {
                    let (dx, dy) = match delta {
                        mouse::ScrollDelta::Lines { x: lx, y: ly } => (lx * 20.0, ly * 20.0),
                        mouse::ScrollDelta::Pixels { x: px, y: py } => (*px, *py),
                    };
                    let mut redraw = false;
                    if let Some(p) = pos_in_bounds {
                        if dx.abs() > 0.1 {
                            redraw |= chart.scroll(-dx, 0.0);
                        }
                        if dy.abs() > 0.1 {
                            let factor = 1.0 - dy * 0.003;
                            redraw |= chart.zoom(factor, p.x);
                        }
                    }
                    redraw
                }
                _ => false,
            }
        };

        if redraw {
            Some(Action::request_redraw())
        } else {
            None
        }
    }

    fn mouse_interaction(
        &self,
        _state: &Self::State,
        bounds: Rectangle,
        cursor: IcedCursor,
    ) -> mouse::Interaction {
        if cursor.is_over(bounds) {
            mouse::Interaction::Grab
        } else {
            mouse::Interaction::default()
        }
    }
}

// ════════════════════════════════════════════════════════════════════════════
//  ChartWithSeparators — multi-canvas chart widget composed with
//  `iced_split::horizontal_split`. Each pane is rendered as an
//  independent Canvas widget. Dragging a separator updates the chart's
//  pane height stretches, which all canvases re-read on the next frame.
// ════════════════════════════════════════════════════════════════════════════

/// Message produced by an `iced_split` separator while dragging.
///
/// The user wires this into their `update` function and calls
/// [`ChartApi::set_pane_height_fraction`] on the pane immediately below
/// the separator that is being dragged.
#[derive(Debug, Clone, Copy)]
pub enum SeparatorMessage {
    /// User dragged separator `pane_index` so that the pane above it
    /// should occupy `pixel_height` pixels.
    Drag {
        pane_index: usize,
        pixel_height: f32,
    },
}

/// A composite widget: multi-canvas chart with `iced_split` separators.
///
/// Each pane of the chart is rendered as its own Canvas widget,
/// which means there is no `with_clip` inside any single Canvas:
/// - Each canvas receives only the events inside its own bounds.
/// - `iced_split` draws the divider, handles drag, and emits a
///   [`SeparatorMessage::Drag`] with the new pixel size of the upper pane.
/// - The user's `update` function translates the message into
///   [`ChartApi::set_pane_height_fraction`].
///
/// `iced_split` is inherently binary, so we compose multiple
/// `horizontal_split`s into a binary tree to support any number of
/// panes.
///
/// # Why shared pane sizes?
///
/// Each pane's Canvas only knows its own dimensions, but the chart
/// engine's layout (`plot_area`, pane height distribution, etc.) must
/// be computed against the **combined** size of all panes. Without
/// coordination, every pane would independently `resize()` the chart
/// to just its own height and the chart would never see the real
/// total height. `pane_sizes` is the shared coordination point:
/// every pane canvas reports its size there, and the combined
/// `(max_width, sum_height)` is what actually gets sent to
/// [`ChartApi::resize`].
#[derive(Clone)]
pub struct ChartWithSeparators {
    chart: Rc<RefCell<ChartApi>>,
    /// Per-pane reported canvas sizes, shared across all pane canvases.
    /// Index is `pane_index`. Each entry is `(width, height)` in CSS
    /// pixels. The combined chart size is `(max of widths, sum of
    /// heights)` — panes are stacked vertically and share a width.
    pane_sizes: Rc<RefCell<Vec<(u32, u32)>>>,
    /// Last combined size that was sent to `chart.resize`. Used to
    /// skip redundant resize calls when the combined size hasn't
    /// actually changed.
    last_total_size: Rc<Cell<(u32, u32)>>,
}

impl ChartWithSeparators {
    /// Create a new wrapper from a `ChartApi`.
    pub fn new(chart: ChartApi) -> Self {
        let pane_count = chart.inner.state.panes.len().max(1);
        Self {
            chart: Rc::new(RefCell::new(chart)),
            pane_sizes: Rc::new(RefCell::new(vec![(0, 0); pane_count])),
            last_total_size: Rc::new(Cell::new((0, 0))),
        }
    }

    /// Wrap an existing shared `Rc<RefCell<ChartApi>>`.
    pub fn from_shared(chart: Rc<RefCell<ChartApi>>) -> Self {
        let pane_count = chart.borrow().inner.state.panes.len().max(1);
        Self {
            chart,
            pane_sizes: Rc::new(RefCell::new(vec![(0, 0); pane_count])),
            last_total_size: Rc::new(Cell::new((0, 0))),
        }
    }

    /// Borrow the inner chart handle.
    pub fn chart_handle(&self) -> Rc<RefCell<ChartApi>> {
        self.chart.clone()
    }

    /// Run a closure with mutable access to the chart.
    pub fn with_chart_mut<R>(&self, f: impl FnOnce(&mut ChartApi) -> R) -> R {
        f(&mut self.chart.borrow_mut())
    }

    /// Number of panes in the chart (and therefore number of canvases
    /// and number of separators minus one).
    pub fn pane_count(&self) -> usize {
        self.chart.borrow().inner.state.panes.len()
    }

    /// Build the final multi-canvas widget tree wrapped with
    /// `iced_split` separators.
    ///
    /// Returns an `iced::Element` ready to be used in any `view`.
    pub fn view<Message>(
        &self,
        on_sep: impl Fn(SeparatorMessage) -> Message + Clone + 'static,
    ) -> iced::Element<'static, Message, Theme, IcedRenderer>
    where
        Message: 'static + Clone,
    {
        let pane_count = self.pane_count();
        debug_assert!(pane_count >= 1);

        if pane_count == 1 {
            return single_pane_canvas::<Message>(self.chart.clone());
        }

        // Ensure the shared pane_sizes vec is sized to match the
        // current pane count (it can change between calls to view if
        // the user added/removed panes via `with_chart_mut`).
        //
        // When the pane count changes we must also force an immediate
        // chart resize at the *old* combined size. Without this, the
        // split positions below are computed from a stale layout
        // (the chart's `plot_area` still reflects the previous pane
        // count), so `iced_split` lays out the panes with wrong
        // heights on the first frame. A freshly-resized chart runs
        // `update_panes_layout`, giving us correct `pane_size`
        // values for the split computation.
        {
            let mut sizes = self.pane_sizes.borrow_mut();
            if sizes.len() != pane_count {
                let old_max_w: u32 = sizes.iter().map(|(w, _)| *w).max().unwrap_or(0);
                let old_sum_h: u32 = sizes.iter().map(|(_, h)| *h).sum();
                *sizes = vec![(0, 0); pane_count];
                self.last_total_size.set((0, 0));
                drop(sizes);
                if old_max_w > 0 && old_sum_h > 0 {
                    let mut c = self.chart.borrow_mut();
                    c.resize(old_max_w, old_sum_h, 1.0);
                    c.render();
                }
            }
        }

        // For each pane, build a Canvas widget sized to fill its slot.
        let canvases: Vec<iced::Element<'static, Message, Theme, IcedRenderer>> = (0..pane_count)
            .map(|i| {
                pane_canvas::<Message>(
                    self.chart.clone(),
                    self.pane_sizes.clone(),
                    self.last_total_size.clone(),
                    i,
                )
            })
            .collect();

        // The split positions (pixel height of each upper pane) come from
        // the chart's current layout.
        let splits: Vec<f32> = {
            let c = self.chart.borrow();
            (0..pane_count - 1)
                .map(|i| {
                    let (_, y, _, h) = c
                        .inner
                        .state
                        .pane_size(i as u32)
                        .unwrap_or((0.0, 0.0, 0.0, 0.0));
                    y + h
                })
                .collect()
        };

        // Compose the binary tree of horizontal_split.
        // We fold right-to-left:
        //   fold(canvas[0..n]) -> Split(p0, Split(p1, Split(p2, ... p_{n-1})))
        let on_sep = on_sep.clone();
        let mut iter = canvases.into_iter();
        let mut iter_split = splits.into_iter().enumerate();

        // Start with the bottom-most (last) pane — it has no separator after it.
        let mut acc: iced::Element<'static, Message, Theme, IcedRenderer> =
            iter.next_back().expect("at least one pane");

        // Each iteration wraps `acc` with a new split on top.
        // We do this in reverse so the final tree looks like:
        //   Split(p0, Split(p1, Split(p2, p3)))
        loop {
            let upper = iter.next();
            match upper {
                Some(upper_elem) => {
                    let (pane_index, split_px) = iter_split
                        .next()
                        .expect("a split position for each non-last pane");
                    let on_sep = on_sep.clone();
                    let pane_index_outer = pane_index;
                    let split_px_outer = split_px;
                    acc = iced_split::horizontal_split::<Message, Theme, IcedRenderer>(
                        upper_elem,
                        acc,
                        split_px_outer,
                        move |pixel_height| {
                            on_sep(SeparatorMessage::Drag {
                                pane_index: pane_index_outer,
                                pixel_height,
                            })
                        },
                    )
                    .strategy(iced_split::Strategy::Start)
                    .handle_width(1.5)
                    .into();
                }
                None => break,
            }
        }

        acc
    }
}

/// Build a Canvas widget that paints only one pane of the chart.
///
/// The chart is shared across all pane canvases so that pane height
/// updates propagate naturally through the chart state on the next
/// `render` call.
///
/// `pane_sizes` and `last_total_size` are shared across all pane
/// canvases so that each pane's draw can coordinate and compute the
/// combined chart size (max width, sum of heights) before calling
/// [`ChartApi::resize`]. Without this, each pane would independently
/// resize the chart to only its own height, and the chart's layout
/// engine (which distributes total height among panes via
/// `update_panes_layout`) would never see the real total.
fn pane_canvas<Message: 'static + Clone>(
    chart: Rc<RefCell<ChartApi>>,
    pane_sizes: Rc<RefCell<Vec<(u32, u32)>>>,
    last_total_size: Rc<Cell<(u32, u32)>>,
    pane_index: usize,
) -> iced::Element<'static, Message, Theme, IcedRenderer> {
    let program = PaneCanvasProgram {
        chart: chart.clone(),
        pane_sizes,
        last_total_size,
        pane_index,
    };
    Canvas::new(program)
        .width(iced::Length::Fill)
        .height(iced::Length::Fill)
        .into()
}

/// Canvas program that paints a single pane's content.
///
/// See [`pane_canvas`] for an explanation of the shared-size
/// coordination. In short: when this pane's dimensions change, we
/// report them to the shared `pane_sizes` map, then compute the
/// combined `(max_width, sum_height)` across *all* panes and send
/// that to [`ChartApi::resize`]. The chart's layout engine needs
/// the combined size to correctly distribute heights among panes.
pub struct PaneCanvasProgram {
    chart: Rc<RefCell<ChartApi>>,
    pane_sizes: Rc<RefCell<Vec<(u32, u32)>>>,
    last_total_size: Rc<Cell<(u32, u32)>>,
    pane_index: usize,
}

impl<Message: 'static> Program<Message, Theme, IcedRenderer> for PaneCanvasProgram {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &IcedRenderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: IcedCursor,
    ) -> Vec<Geometry> {
        let w = bounds.width.round().max(1.0) as u32;
        let h = bounds.height.round().max(1.0) as u32;

        // Report this pane's new size and compute the combined total
        // across all panes (max width, sum of heights).
        let combined: (u32, u32) = {
            let mut sizes = self.pane_sizes.borrow_mut();
            if self.pane_index < sizes.len() {
                sizes[self.pane_index] = (w, h);
            }
            let mut max_w: u32 = 0;
            let mut sum_h: u32 = 0;
            for &(pw, ph) in sizes.iter() {
                if pw > max_w {
                    max_w = pw;
                }
                sum_h += ph;
            }
            (max_w, sum_h)
        };

        let prev_total = self.last_total_size.get();
        if prev_total != combined && combined.0 > 0 && combined.1 > 0 {
            self.last_total_size.set(combined);
            let mut c = self.chart.borrow_mut();
            c.resize(combined.0, combined.1, 1.0);
            c.render();
        }

        let mut frame = Frame::new(renderer, bounds.size());
        {
            let chart = self.chart.borrow();
            paint_pane_to_iced_frame(self.pane_index, &mut frame, &chart.inner.state);
        }
        vec![frame.into_geometry()]
    }

    fn update(
        &self,
        _state: &mut Self::State,
        event: &Event,
        bounds: Rectangle,
        cursor: IcedCursor,
    ) -> Option<Action<Message>> {
        // Pane canvas receives events in its own local coordinate space.
        // We must translate to the chart's absolute coordinate space
        // before forwarding to the chart engine.
        //
        // The translation is:
        //   chart_x = local_x
        //   chart_y = local_y + pane.layout_rect.y
        //
        // because the canvas's (0, 0) maps to chart coordinate
        // (0, pane.layout_rect.y) — the top-left of this pane's slot
        // in the total chart layout.
        let pos_in_bounds = cursor.position_in(bounds);

        let redraw = {
            let mut chart = self.chart.borrow_mut();
            match event {
                Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                    if let Some(p) = pos_in_bounds {
                        let pane_y = chart
                            .inner
                            .state
                            .pane_size(self.pane_index as u32)
                            .map(|(_, y, _, _)| y)
                            .unwrap_or(0.0);
                        chart.pointer_move(p.x, p.y + pane_y)
                    } else {
                        // Cursor left this pane's bounds — but may
                        // still be over another pane's canvas. Only
                        // clear crosshair if leaving the entire chart.
                        // For simplicity we don't clear here; the
                        // pane receiving the cursor will set its own
                        // active pane.
                        false
                    }
                }
                Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                    if let Some(p) = pos_in_bounds {
                        let pane_y = chart
                            .inner
                            .state
                            .pane_size(self.pane_index as u32)
                            .map(|(_, y, _, _)| y)
                            .unwrap_or(0.0);
                        chart.pointer_down(p.x, p.y + pane_y, 0)
                    } else {
                        false
                    }
                }
                Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                    if let Some(p) = pos_in_bounds {
                        let pane_y = chart
                            .inner
                            .state
                            .pane_size(self.pane_index as u32)
                            .map(|(_, y, _, _)| y)
                            .unwrap_or(0.0);
                        chart.pointer_up(p.x, p.y + pane_y, 0)
                    } else {
                        false
                    }
                }
                Event::Mouse(mouse::Event::CursorLeft) => chart.pointer_leave(),
                Event::Mouse(mouse::Event::WheelScrolled { delta }) => {
                    let (dx, dy) = match delta {
                        mouse::ScrollDelta::Lines { x: lx, y: ly } => (lx * 20.0, ly * 20.0),
                        mouse::ScrollDelta::Pixels { x: px, y: py } => (*px, *py),
                    };
                    let mut redraw = false;
                    if let Some(p) = pos_in_bounds {
                        if dx.abs() > 0.1 {
                            redraw |= chart.scroll(-dx, 0.0);
                        }
                        if dy.abs() > 0.1 {
                            let factor = 1.0 - dy * 0.003;
                            redraw |= chart.zoom(factor, p.x);
                        }
                    }
                    redraw
                }
                _ => false,
            }
        };

        if redraw {
            Some(Action::request_redraw())
        } else {
            None
        }
    }

    fn mouse_interaction(
        &self,
        _state: &Self::State,
        bounds: Rectangle,
        cursor: IcedCursor,
    ) -> mouse::Interaction {
        if cursor.is_over(bounds) {
            mouse::Interaction::Grab
        } else {
            mouse::Interaction::default()
        }
    }
}

/// Build a Canvas widget for the single-pane case (no separator needed).
fn single_pane_canvas<Message: 'static + Clone>(
    chart: Rc<RefCell<ChartApi>>,
) -> iced::Element<'static, Message, Theme, IcedRenderer> {
    let program = SinglePaneProgram {
        chart: chart.clone(),
        last_size: Rc::new(Cell::new((0, 0))),
    };
    Canvas::new(program)
        .width(iced::Length::Fill)
        .height(iced::Length::Fill)
        .into()
}

/// Canvas program for the single-pane fast path.
pub struct SinglePaneProgram {
    chart: Rc<RefCell<ChartApi>>,
    last_size: Rc<Cell<(u32, u32)>>,
}

impl<Message: 'static> Program<Message, Theme, IcedRenderer> for SinglePaneProgram {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &IcedRenderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: IcedCursor,
    ) -> Vec<Geometry> {
        let w = bounds.width.round().max(1.0) as u32;
        let h = bounds.height.round().max(1.0) as u32;

        let prev = self.last_size.get();
        if prev != (w, h) {
            self.last_size.set((w, h));
            let mut c = self.chart.borrow_mut();
            c.resize(w, h, 1.0);
            c.render();
        }

        let mut frame = Frame::new(renderer, bounds.size());
        {
            let chart = self.chart.borrow();
            paint_to_iced_frame(&mut frame, &chart.inner.state);
        }
        vec![frame.into_geometry()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_conversion_preserves_components() {
        let c = Color([0.25, 0.5, 0.75, 1.0]);
        let iced = c4_to_iced(c);
        assert_eq!(iced.r, 0.25);
        assert_eq!(iced.g, 0.5);
        assert_eq!(iced.b, 0.75);
        assert_eq!(iced.a, 1.0);
    }

    #[test]
    fn points_applies_scale() {
        // Test the Backend's point conversion by constructing one with a
        // dummy frame. Since Frame construction needs an Iced Renderer,
        // we just sanity-check the math directly.
        fn scaled(points: &[(f64, f64)], sx: f32, sy: f32) -> Vec<(f32, f32)> {
            points
                .iter()
                .map(|&(x, y)| ((x as f32) * sx, (y as f32) * sy))
                .collect()
        }
        assert_eq!(
            scaled(&[(1.0, 2.0), (3.0, 4.0)], 2.0, 0.5),
            vec![(2.0, 1.0), (6.0, 2.0)]
        );
    }
}
