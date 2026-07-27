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
use iced::widget::canvas::{Action, Canvas, Event, Frame, Geometry, Program};
use iced::{mouse, Rectangle, Renderer as IcedRenderer, Theme};

use crate::chart::color::Palette;
use crate::widget::backend::c4_to_iced;
use crate::ChartApi;

mod backend;
mod programs;

pub use backend::{snap_x, snap_y, IcedBackend};

use backend::paint_to_iced_frame;
use programs::pane_canvas;

// ════════════════════════════════════════════════════════════════════════════
//  Separator styling — make pane splits invisible when not focused
// ════════════════════════════════════════════════════════════════════════════

/// The handle area width for drag-hit testing.
/// The visual separator is styled to be invisible (zero width,
/// matching chart background) when not focused, so the gap between
/// panes is only a touch target — not a visible artifact.
///
/// NOTE: iced_split's layout always consumes `handle_width` pixels
/// from the child widgets. Keep this as small as possible so the
/// gap between panes is minimal. A value of 1.0 is the smallest
/// usable drag target; below that, hit-testing becomes unreliable.
const SEPARATOR_HANDLE_WIDTH: f32 = 1.0;

/// Returns a style for the iced_split separator that:
/// - matches the chart background color when not focused (invisible)
/// - shows a visible indicator when being dragged
fn separator_style(_theme: &Theme) -> iced_split::Style {
    let bg = c4_to_iced(Palette::Background.color());
    let hover = c4_to_iced(Palette::Axis.color());

    iced_split::Style {
        unfocused: iced_split::StyleSheet {
            color: bg,
            width: 0.0,
            radius: 0.0.into(),
        },
        focused: iced_split::StyleSheet {
            color: hover,
            width: 1.0,
            radius: 0.0.into(),
        },
        snap: true,
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

        let prev = self.last_size.get();
        if prev != (w, h) {
            self.last_size.set((w, h));
            self.chart.borrow_mut().resize(w, h, 1.0);
            self.chart.borrow_mut().render();
        }

        let mut frame = Frame::new(renderer, bounds.size());

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
pub struct ChartWidget {
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

impl ChartWidget {
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

        let splits: Vec<f32> = {
            let c = self.chart.borrow();
            (0..pane_count - 1)
                .map(|i| {
                    let (_, _, _, h) = c
                        .inner
                        .state
                        .pane_size(i as u32)
                        .unwrap_or((0.0, 0.0, 0.0, 0.0));
                    h
                })
                .collect()
        };

        let on_sep = on_sep.clone();
        let mut iter = canvases.into_iter();
        let mut iter_split = splits.into_iter().enumerate();

        let mut acc: iced::Element<'static, Message, Theme, IcedRenderer> =
            iter.next_back().expect("at least one pane");

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
                    .handle_width(SEPARATOR_HANDLE_WIDTH)
                    .style(separator_style)
                    .into();
                }
                None => break,
            }
        }

        acc
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chart::color::Color;

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
