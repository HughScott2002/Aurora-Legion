//! Live keyboard preview: a keycap grid over four glowing zones, drawn with
//! cairo on a `gtk::DrawingArea`. The one place custom drawing is allowed;
//! everything else in the GUI is stock libadwaita.

use std::{cell::Cell, rc::Rc};

use relm4::gtk::{self, cairo, prelude::*};

const PREVIEW_HEIGHT_PX: i32 = 130;
const ZONE_COUNT: usize = 4;

/// Keycap grid: uniform columns except a wide spacebar in the last row.
const KEY_COLUMNS: usize = 18;
const KEY_ROWS: usize = 5;
const KEY_GAP_PX: f64 = 3.0;
const KEY_CORNER_RADIUS_PX: f64 = 2.5;

/// How many keys each backlight zone covers, from the key lists the
/// daemon's ripple effect uses. The zones are not equal quarters: zone 2
/// is the widest and zone 4 is the numpad. Splitting the drawing evenly
/// put the boundaries in the wrong place.
const ZONE_KEY_COUNTS: [f64; ZONE_COUNT] = [24.0, 29.0, 25.0, 18.0];

pub struct KeyboardPreview {
    pub root: gtk::Widget,
    drawing_area: gtk::DrawingArea,
    zone_colors: Rc<Cell<[[u8; 3]; ZONE_COUNT]>>,
}

impl KeyboardPreview {
    pub fn new() -> Self {
        let zone_colors: Rc<Cell<[[u8; 3]; ZONE_COUNT]>> = Rc::new(Cell::new([[0; 3]; ZONE_COUNT]));

        // Presentation role: the preview is decorative; the zone color
        // buttons below carry the same information accessibly.
        let drawing_area = gtk::DrawingArea::builder()
            .content_height(PREVIEW_HEIGHT_PX)
            .hexpand(true)
            .accessible_role(gtk::AccessibleRole::Presentation)
            .build();

        let colors_for_draw = zone_colors.clone();
        drawing_area.set_draw_func(move |_area, context, width, height| {
            let colors = colors_for_draw.get();
            draw_keyboard(context, f64::from(width), f64::from(height), &colors);
        });

        // A card container clips the drawing to rounded corners and matches
        // the boxed-list styling of the groups below it.
        let card = gtk::Box::new(gtk::Orientation::Vertical, 0);
        card.add_css_class("card");
        card.set_overflow(gtk::Overflow::Hidden);
        card.append(&drawing_area);

        Self {
            root: card.upcast(),
            drawing_area,
            zone_colors,
        }
    }

    pub fn set_colors(&self, colors: [[u8; 3]; ZONE_COUNT]) {
        if self.zone_colors.get() == colors {
            return;
        }
        self.zone_colors.set(colors);
        self.drawing_area.queue_draw();
    }
}

fn draw_keyboard(
    context: &cairo::Context,
    width: f64,
    height: f64,
    colors: &[[u8; 3]; ZONE_COUNT],
) {
    // The laptop deck: always dark, independent of the GTK theme, because
    // it depicts the physical keyboard.
    context.set_source_rgb(0.075, 0.075, 0.09);
    context.rectangle(0.0, 0.0, width, height);
    if context.fill().is_err() {
        return;
    }

    let deck_margin = 14.0;
    let grid_left = deck_margin;
    let grid_top = deck_margin;
    let grid_width = width - deck_margin * 2.0;
    let grid_height = height - deck_margin * 2.0;

    // Backlight glow: one radial gradient per zone, drawn under the keys so
    // it shines through the gaps between keycaps.
    let zone_bounds = zone_bounds(grid_left, grid_width);
    for (zone_index, zone_color) in colors.iter().enumerate() {
        let (red, green, blue) = color_to_rgb_f64(zone_color);

        let (zone_left, zone_right) = zone_bounds[zone_index];
        let zone_width = zone_right - zone_left;

        let center_x = zone_left + zone_width / 2.0;
        let center_y = grid_top + grid_height / 2.0;
        let glow_radius = zone_width * 0.85;

        let gradient =
            cairo::RadialGradient::new(center_x, center_y, 4.0, center_x, center_y, glow_radius);
        gradient.add_color_stop_rgba(0.0, red, green, blue, 0.8);
        gradient.add_color_stop_rgba(1.0, red, green, blue, 0.05);
        if context.set_source(&gradient).is_err() {
            return;
        }
        context.rectangle(grid_left, grid_top, grid_width, grid_height);
        if context.fill().is_err() {
            return;
        }
    }

    // Keycaps: a uniform grid, each key tinted by the zone its center falls
    // in. The last row gets a wide spacebar like a real keyboard.
    let column_width = (grid_width - KEY_GAP_PX * (KEY_COLUMNS as f64 - 1.0)) / KEY_COLUMNS as f64;
    let row_height = (grid_height - KEY_GAP_PX * (KEY_ROWS as f64 - 1.0)) / KEY_ROWS as f64;

    for row_index in 0..KEY_ROWS {
        let key_y = grid_top + row_index as f64 * (row_height + KEY_GAP_PX);
        let is_bottom_row = row_index == KEY_ROWS - 1;

        let mut column_index = 0;
        while column_index < KEY_COLUMNS {
            // Spacebar: columns 5..=10 of the bottom row as one wide key.
            let spanned_columns = if is_bottom_row && column_index == 5 {
                6
            } else {
                1
            };

            let key_x = grid_left + column_index as f64 * (column_width + KEY_GAP_PX);
            let key_width =
                column_width * spanned_columns as f64 + KEY_GAP_PX * (spanned_columns as f64 - 1.0);

            let key_center_x = key_x + key_width / 2.0;
            let zone_index = zone_index_for_x(key_center_x, &zone_bounds);
            let (red, green, blue) = color_to_rgb_f64(&colors[zone_index]);

            // Keycap surface: the zone color over a dark cap, so black
            // zones show unlit dark keys instead of vanishing.
            rounded_rectangle(
                context,
                key_x,
                key_y,
                key_width,
                row_height,
                KEY_CORNER_RADIUS_PX,
            );
            context.set_source_rgb(0.12 + red * 0.75, 0.12 + green * 0.75, 0.12 + blue * 0.75);
            if context.fill().is_err() {
                return;
            }

            column_index += spanned_columns;
        }
    }
}

/// Left and right edge of each zone, weighted by how many keys it covers.
fn zone_bounds(grid_left: f64, grid_width: f64) -> [(f64, f64); ZONE_COUNT] {
    let mut total_keys = 0.0;
    for key_count in ZONE_KEY_COUNTS {
        total_keys += key_count;
    }

    let mut bounds: [(f64, f64); ZONE_COUNT] = [(0.0, 0.0); ZONE_COUNT];
    let mut edge = grid_left;
    for (zone_index, key_count) in ZONE_KEY_COUNTS.iter().enumerate() {
        let zone_width = grid_width * key_count / total_keys;
        bounds[zone_index] = (edge, edge + zone_width);
        edge += zone_width;
    }

    // Absorb rounding into the last edge so the zones tile the grid exactly.
    bounds[ZONE_COUNT - 1].1 = grid_left + grid_width;
    bounds
}

fn zone_index_for_x(x: f64, bounds: &[(f64, f64); ZONE_COUNT]) -> usize {
    for (zone_index, (_left, right)) in bounds.iter().enumerate() {
        if x < *right {
            return zone_index;
        }
    }

    ZONE_COUNT - 1
}

fn color_to_rgb_f64(color: &[u8; 3]) -> (f64, f64, f64) {
    let red = f64::from(color[0]) / 255.0;
    let green = f64::from(color[1]) / 255.0;
    let blue = f64::from(color[2]) / 255.0;
    (red, green, blue)
}

fn rounded_rectangle(
    context: &cairo::Context,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    radius: f64,
) {
    let degrees = std::f64::consts::PI / 180.0;

    context.new_sub_path();
    context.arc(x + width - radius, y + radius, radius, -90.0 * degrees, 0.0);
    context.arc(
        x + width - radius,
        y + height - radius,
        radius,
        0.0,
        90.0 * degrees,
    );
    context.arc(
        x + radius,
        y + height - radius,
        radius,
        90.0 * degrees,
        180.0 * degrees,
    );
    context.arc(
        x + radius,
        y + radius,
        radius,
        180.0 * degrees,
        270.0 * degrees,
    );
    context.close_path();
}
