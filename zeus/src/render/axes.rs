use super::{BG_COLOR, LABEL_FONT_SIZE, TEXT_COLOR};
use crate::chart::chart_state::ChartState;
use crate::chart::tick_marks::{generate_price_ticks, TickMark};
use crate::widget::{snap_x, snap_y, IcedBackend};

pub fn draw_y_axis<'a>(
    b: &mut IcedBackend<'a>,
    state: &ChartState,
    layout: &crate::chart::chart_model::ChartLayout,
) {
    let gutter_x = (layout.plot_area.x + layout.plot_area.width) as f64;
    let gutter_w = layout.margins.right as f64;
    b.fill_rect(gutter_x, 0.0, gutter_w, layout.height as f64, BG_COLOR);

    let x_start = (layout.plot_area.x + layout.plot_area.width + 5.0) as f64;
    let sf = layout.scale_factor;

    for pane in &state.panes {
        let price_ticks = generate_price_ticks(&pane.price_scale, &pane.layout_rect);
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
    }
}

pub fn draw_x_axis<'a>(
    b: &mut IcedBackend<'a>,
    time_ticks: &[TickMark],
    layout: &crate::chart::chart_model::ChartLayout,
) {
    let plot = &layout.plot_area;

    let gutter_y = (plot.y + plot.height) as f64;
    let gutter_h = layout.margins.bottom as f64;
    b.fill_rect(0.0, gutter_y, layout.width as f64, gutter_h, BG_COLOR);

    let y_start = (plot.y + plot.height + 5.0) as f64;
    let sf = layout.scale_factor;

    for tick in time_ticks {
        let x = snap_x(tick.coord as f64, sf);

        let label_w = b.measure_text(&tick.label, LABEL_FONT_SIZE);
        b.draw_text(
            &tick.label,
            x - label_w / 2.0,
            y_start + 12.0,
            LABEL_FONT_SIZE,
            TEXT_COLOR,
        );
    }
}
