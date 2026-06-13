use crate::core::types::Pin;

/// Layout constants for node rendering.
/// All values are unscaled — multiply by zoom_level before converting to pixels.

pub const HEADER_H: f32 = 28.0;
pub const SEP_H: f32 = 2.0;
pub const BODY_PAD: f32 = 8.0;
pub const PIN_ROW_H: f32 = 18.0;
pub const PIN_GAP: f32 = 4.0;
pub const PIN_SIZE: f32 = 12.0;
pub const NODE_BASE_W: f32 = 240.0;
pub const NODE_PREVIEW_W: f32 = 320.0;
pub const TEXTURE_PREVIEW_SIZE: f32 = 52.0;
pub const TEXTURE_PREVIEW_GAP: f32 = 8.0;

/// Grid snap interval (graph-space units). Node dimensions are rounded up to
/// the nearest multiple of this value so they align with grid snapping.
pub const GRID_SNAP: f32 = 10.0;

pub const NODE_BASE_H: f32 = HEADER_H + SEP_H + BODY_PAD * 2.0;

pub fn pin_supports_texture_preview(pin: &Pin) -> bool {
    pin.data_type.is_texture_previewable()
}

pub fn node_has_texture_preview(outputs: &[Pin]) -> bool {
    outputs.iter().any(pin_supports_texture_preview)
}

pub fn node_width_for_pins(outputs: &[Pin]) -> f32 {
    if node_has_texture_preview(outputs) {
        NODE_PREVIEW_W
    } else {
        NODE_BASE_W
    }
}

pub fn node_pin_row_count(inputs: &[Pin], outputs: &[Pin]) -> usize {
    inputs.len().max(outputs.len()).max(1)
}

pub fn pin_row_height(input_pin: Option<&Pin>, output_pin: Option<&Pin>) -> f32 {
    let input_h = if input_pin.is_some() { PIN_ROW_H } else { 0.0 };
    let output_h = match output_pin {
        Some(pin) if pin_supports_texture_preview(pin) => TEXTURE_PREVIEW_SIZE,
        Some(_) => PIN_ROW_H,
        None => 0.0,
    };

    input_h.max(output_h).max(PIN_ROW_H)
}

pub fn pin_row_offset(inputs: &[Pin], outputs: &[Pin], row: usize) -> f32 {
    let mut offset = 0.0;
    for idx in 0..row {
        if idx > 0 {
            offset += PIN_GAP;
        }
        offset += pin_row_height(inputs.get(idx), outputs.get(idx));
    }
    offset
}

pub fn pin_row_center_y(inputs: &[Pin], outputs: &[Pin], row: usize) -> f32 {
    pin_row_offset(inputs, outputs, row) + pin_row_height(inputs.get(row), outputs.get(row)) * 0.5
}

pub fn node_height_for_pins(inputs: &[Pin], outputs: &[Pin]) -> f32 {
    let rows = node_pin_row_count(inputs, outputs);
    let mut body_h = 0.0;
    for row in 0..rows {
        if row > 0 {
            body_h += PIN_GAP;
        }
        body_h += pin_row_height(inputs.get(row), outputs.get(row));
    }
    NODE_BASE_H + body_h
}

pub fn node_height_for_pin_rows(pin_rows: usize) -> f32 {
    let rows = pin_rows.max(1) as f32;
    NODE_BASE_H + rows * PIN_ROW_H + ((rows - 1.0).max(0.0)) * PIN_GAP
}

/// Round `value` up to the nearest multiple of `GRID_SNAP`.
pub fn snap_to_grid(value: f32) -> f32 {
    (value / GRID_SNAP).ceil() * GRID_SNAP
}
