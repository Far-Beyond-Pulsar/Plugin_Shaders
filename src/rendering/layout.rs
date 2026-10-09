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

/// Live texture previews on node pins are a debugging aid, off by default.
/// Each one compiles its own shader and renders every frame, so the editor
/// only pays for them when they are explicitly switched on (the toolbar's
/// debug toggle). Everything that depends on previews, node width and row
/// height, label placement, the preview requests themselves and the
/// continuous redraw they cause, goes through
/// [`pin_supports_texture_preview`], so this one flag controls all of it.
static TEXTURE_PREVIEWS_ENABLED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

pub fn texture_previews_enabled() -> bool {
    TEXTURE_PREVIEWS_ENABLED.load(std::sync::atomic::Ordering::Relaxed)
}

/// Switch pin texture previews on or off. Node sizes depend on this, so
/// callers follow it with [`relayout_graph`] for every open graph.
pub fn set_texture_previews_enabled(enabled: bool) {
    TEXTURE_PREVIEWS_ENABLED.store(enabled, std::sync::atomic::Ordering::Relaxed);
}

/// Whether `pin` shows a live texture preview right now.
pub fn pin_supports_texture_preview(pin: &Pin) -> bool {
    texture_previews_enabled() && pin.data_type.is_texture_previewable()
}

/// Recompute every node's size from its pins, after the preview setting
/// changed. Reroute nodes keep their fixed size.
pub fn relayout_graph(graph: &mut crate::BlueprintGraph) {
    for node in &mut graph.nodes {
        if node.node_type == crate::NodeType::Reroute {
            continue;
        }
        node.size = crate::Size::new(
            node_width_for_pins(&node.outputs),
            node_height_for_pins(&node.inputs, &node.outputs),
        );
    }
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

#[cfg(test)]
mod preview_toggle_tests {
    use super::*;
    use crate::core::types::{PinDataType, PinType};

    fn color_pin() -> Pin {
        Pin {
            id: "result".into(),
            name: "result".into(),
            pin_type: PinType::Output,
            data_type: PinDataType::from_type_str("vec4<f32>"),
        }
    }

    /// Previews are a debugging aid: by default a colour pin is an ordinary
    /// pin row on an ordinary-width node, so nothing is reserved, compiled or
    /// redrawn for it. Switching them on widens the node and grows the row;
    /// switching off restores the compact layout.
    #[test]
    fn previews_are_off_by_default_and_the_toggle_drives_the_layout() {
        let outputs = [color_pin()];
        assert!(!texture_previews_enabled(), "off unless explicitly enabled");
        assert!(!pin_supports_texture_preview(&outputs[0]));
        assert_eq!(node_width_for_pins(&outputs), NODE_BASE_W);
        assert_eq!(pin_row_height(None, Some(&outputs[0])), PIN_ROW_H);

        set_texture_previews_enabled(true);
        let (width, row) = (
            node_width_for_pins(&outputs),
            pin_row_height(None, Some(&outputs[0])),
        );
        set_texture_previews_enabled(false);
        assert_eq!(width, NODE_PREVIEW_W);
        assert_eq!(row, TEXTURE_PREVIEW_SIZE);
        assert_eq!(node_width_for_pins(&outputs), NODE_BASE_W);
    }
}
