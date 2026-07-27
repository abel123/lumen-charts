use std::cell::{Cell, RefCell};
use std::rc::Rc;

use iced::mouse::Cursor as IcedCursor;
use iced::widget::canvas::{Action, Canvas, Event, Frame, Geometry, Program};
use iced::{
    mouse, Rectangle, Renderer as IcedRenderer, Theme,
};

use crate::ChartApi;

use super::backend::{paint_pane_to_iced_frame, paint_to_iced_frame};

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
pub(crate) fn pane_canvas<Message: 'static + Clone>(
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

/// Canvas program for the single-pane fast path.
pub struct SinglePaneProgram {
    pub(super) chart: Rc<RefCell<ChartApi>>,
    pub(super) last_size: Rc<Cell<(u32, u32)>>,
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