//! Connection operations - dragging and managing connections between nodes

use crate::core::types::{BlueprintNode, Connection, NodeType};
use crate::editor::workspace_panels::GraphCanvasPanel;
use gpui::*;
use crate::core::types::PinDataType as GraphDataType;

/// Connection drag state
#[derive(Clone, Debug)]
pub struct ConnectionDrag {
    pub source_node: String,
    pub source_pin: String,
    pub source_pin_type: GraphDataType,
    pub current_mouse_pos: Point<f32>,
    pub target_pin: Option<(String, String)>,
}

impl GraphCanvasPanel {
    /// Start dragging a connection from a pin
    pub fn start_connection_drag_from_pin(
        &mut self,
        node_id: String,
        pin_id: String,
        mouse_pos: Point<f32>,
        cx: &mut Context<Self>,
    ) {
        if let Some(node) = self.graph.nodes.iter().find(|n| n.id == node_id) {
            if let Some(pin) = node.outputs.iter().find(|p| p.id == pin_id) {
                tracing::info!(
                    "Starting connection drag from pin {} on node {}",
                    pin_id,
                    node_id
                );
                self.dragging_connection = Some(ConnectionDrag {
                    source_node: node_id,
                    source_pin: pin_id,
                    source_pin_type: pin.data_type.clone(),
                    current_mouse_pos: mouse_pos,
                    target_pin: None,
                });

                // Close tooltips when starting connection drag
                // Tooltip removed - use node picker instead
                cx.notify();
            }
        }
    }

    /// Update connection drag position
    pub fn update_connection_drag(&mut self, mouse_pos: Point<f32>, cx: &mut Context<Self>) {
        if let Some(ref mut drag) = self.dragging_connection {
            drag.current_mouse_pos = mouse_pos;
            cx.notify();
        }
    }

    /// Cancel connection drag
    pub fn cancel_connection_drag(&mut self, cx: &mut Context<Self>) {
        self.dragging_connection = None;
        cx.notify();
    }

    /// Set connection target (hovering over a pin)
    pub fn set_connection_target(
        &mut self,
        target_node_id: Option<String>,
        target_pin_id: Option<String>,
        cx: &mut Context<Self>,
    ) {
        if let Some(ref mut drag) = self.dragging_connection {
            drag.target_pin = target_node_id.zip(target_pin_id);
            cx.notify();
        }
    }

    /// Complete connection on a pin
    pub fn complete_connection_on_pin(
        &mut self,
        node_id: String,
        pin_id: String,
        cx: &mut Context<Self>,
    ) {
        if let Some(drag) = self.dragging_connection.take() {
            // Validate connection
            if let Some(node) = self.graph.nodes.iter().find(|n| n.id == node_id) {
                if let Some(pin) = node.inputs.iter().find(|p| p.id == pin_id) {
                    // Clone pin data type before mutable operations
                    let pin_data_type = pin.data_type.clone();

                    // Check compatibility and not same node
                    if drag.source_node == node_id {
                        tracing::info!("Cannot connect a node to itself");
                    } else if super::compatibility::are_types_compatible(
                        &drag.source_pin_type,
                        &pin_data_type,
                    ) {
                        // Check if source or target is a reroute node
                        let source_is_reroute =
                            self.graph.nodes.iter().any(|n| {
                                n.id == drag.source_node && n.node_type == NodeType::Reroute
                            });
                        let target_is_reroute = self
                            .graph
                            .nodes
                            .iter()
                            .any(|n| n.id == node_id && n.node_type == NodeType::Reroute);

                        // Remove old connections based on pin types
                        if drag.source_pin_type == GraphDataType::execution() || source_is_reroute {
                            tracing::info!(
                                "Removing old connection from source {}:{}",
                                drag.source_node,
                                drag.source_pin
                            );
                            self.graph.connections.retain(|conn| {
                                !(conn.source_node == drag.source_node
                                    && conn.source_pin == drag.source_pin)
                            });
                        }

                        if drag.source_pin_type == GraphDataType::execution()
                            || target_is_reroute
                            || pin_data_type != GraphDataType::execution()
                        {
                            tracing::info!(
                                "Removing old connection to target {}:{}",
                                node_id,
                                pin_id
                            );
                            self.graph.connections.retain(|conn| {
                                !(conn.target_node == node_id && conn.target_pin == pin_id)
                            });
                        }

                        tracing::info!(
                            "Creating connection from {}:{} to {}:{}",
                            drag.source_node, drag.source_pin, node_id, pin_id
                        );

                        let connection_type = if pin_data_type == GraphDataType::execution() {
                            ui::graph::ConnectionType::Execution
                        } else {
                            ui::graph::ConnectionType::Data
                        };

                        let connection = Connection {
                            id: uuid::Uuid::new_v4().to_string(),
                            source_node: drag.source_node.clone(),
                            source_pin: drag.source_pin.clone(),
                            target_node: node_id.clone(),
                            target_pin: pin_id.clone(),
                            connection_type,
                        };

                        let mut cmd =
                            crate::features::undo::AddConnectionCommand::new(connection.clone());
                        cmd.execute(self, cx);
                        self.push_undo_command(crate::features::undo::Command::AddConnection(cmd));

                        tracing::info!("Connection created successfully!");

                        // Propagate types through reroute nodes
                        if target_is_reroute {
                            self.propagate_reroute_types(node_id.clone(), drag.source_pin_type, cx);
                        } else if source_is_reroute {
                            self.propagate_reroute_types(
                                drag.source_node.clone(),
                                pin_data_type,
                                cx,
                            );
                        }

                        cx.notify();

                    // ── Type-mismatch: try auto-conversion ─────────────────────
                    } else if let Some(path) = super::compatibility::get_conversion_path(
                        &drag.source_pin_type,
                        &pin_data_type,
                    ) {
                        // Find positions of source and target nodes for layout
                        let source_pos = self
                            .graph
                            .nodes
                            .iter()
                            .find(|n| n.id == drag.source_node)
                            .map(|n| n.position)
                            .unwrap_or(gpui::Point::new(0.0, 0.0));
                        let target_pos = self
                            .graph
                            .nodes
                            .iter()
                            .find(|n| n.id == node_id)
                            .map(|n| n.position)
                            .unwrap_or(gpui::Point::new(0.0, 0.0));

                        // Remove existing connections to the target pin (like any
                        // real node-placement would do).
                        self.graph.connections.retain(|conn| {
                            !(conn.target_node == node_id && conn.target_pin == pin_id)
                        });

                        let mid_x = (source_pos.x + target_pos.x) / 2.0;
                        let mid_y = (source_pos.y + target_pos.y) / 2.0;
                        let step_offset = 60.0 * (path.len() as f32 + 1.0) / 2.0;

                        let mut prev_node = drag.source_node.clone();
                        let mut prev_pin = drag.source_pin.clone();

                        for (step_idx, (conv_node_type, _from_ty, _to_ty)) in path.iter().enumerate() {
                            let conv_id = format!(
                                "__auto_conv_{}__{}__",
                                conv_node_type,
                                uuid::Uuid::new_v4()
                            );

                            // Build a proper node from its definition — this gives
                            // correct sizing, pin layout, category colouring, and
                            // avoids the reroute/pill rendering path entirely.
                            let defs = crate::core::definitions::NodeDefinitions::load();
                            let def = defs.get_node_definition(conv_node_type);
                            let mut conv_node = match def {
                                Some(d) => BlueprintNode::from_definition(
                                    d,
                                    gpui::Point::new(
                                        mid_x,
                                        mid_y - step_offset + 60.0 * step_idx as f32,
                                    ),
                                ),
                                None => continue,
                            };
                            // Override the auto-generated ID so we can wire it up.
                            conv_node.id = conv_id.clone();

                            let input_pin_name = conv_node
                                .inputs
                                .first()
                                .map(|p| p.id.clone())
                                .unwrap_or_else(|| "input".to_string());

                            let mut node_cmd = crate::features::undo::AddNodeCommand::new(conv_node);
                            node_cmd.execute(self, cx);
                            self.push_undo_command(
                                crate::features::undo::Command::AddNode(node_cmd),
                            );

                            // Wire previous step → conversion input
                            let conn = Connection {
                                id: uuid::Uuid::new_v4().to_string(),
                                source_node: prev_node,
                                source_pin: prev_pin,
                                target_node: conv_id.clone(),
                                target_pin: input_pin_name,
                                connection_type: ui::graph::ConnectionType::Data,
                            };
                            let mut conn_cmd =
                                crate::features::undo::AddConnectionCommand::new(conn);
                            conn_cmd.execute(self, cx);
                            self.push_undo_command(
                                crate::features::undo::Command::AddConnection(conn_cmd),
                            );

                            prev_node = conv_id;
                            prev_pin = "result".to_string();
                        }

                        // Wire last conversion output → original target
                        let final_conn = Connection {
                            id: uuid::Uuid::new_v4().to_string(),
                            source_node: prev_node,
                            source_pin: prev_pin,
                            target_node: node_id.clone(),
                            target_pin: pin_id.clone(),
                            connection_type: ui::graph::ConnectionType::Data,
                        };
                        let mut final_cmd =
                            crate::features::undo::AddConnectionCommand::new(final_conn);
                        final_cmd.execute(self, cx);
                        self.push_undo_command(
                            crate::features::undo::Command::AddConnection(final_cmd),
                        );

                        tracing::info!(
                            "Auto-conversion inserted: {} → {} via {} steps",
                            drag.source_pin_type.type_name,
                            pin_data_type.type_name,
                            path.len()
                        );
                        cx.notify();
                    } else {
                        tracing::info!("Incompatible pin types or same node");
                    }
                }
            }
        }
    }

    /// Disconnect a pin
    pub fn disconnect_pin(&mut self, node_id: String, pin_id: String, cx: &mut Context<Self>) {
        // Collect connections to delete
        let connections_to_delete: Vec<_> = self
            .graph
            .connections
            .iter()
            .filter(|conn| {
                (conn.source_node == node_id && conn.source_pin == pin_id)
                    || (conn.target_node == node_id && conn.target_pin == pin_id)
            })
            .cloned()
            .collect();

        if !connections_to_delete.is_empty() {
            // Create batch command if multiple connections
            if connections_to_delete.len() == 1 {
                let mut cmd = crate::features::undo::DeleteConnectionCommand::new(
                    connections_to_delete[0].clone(),
                );
                cmd.execute(self, cx);
                self.push_undo_command(crate::features::undo::Command::DeleteConnection(cmd));
            } else {
                let mut batch =
                    crate::features::undo::BatchCommand::new("Disconnect pin".to_string());
                for connection in connections_to_delete {
                    batch.add_command(crate::features::undo::Command::DeleteConnection(
                        crate::features::undo::DeleteConnectionCommand::new(connection),
                    ));
                }
                batch.execute(self, cx);
                self.push_undo_command(crate::features::undo::Command::Batch(batch));
            }
        }
    }

    /// Propagate types through connected reroute nodes
    pub fn propagate_reroute_types(
        &mut self,
        start_node_id: String,
        data_type: GraphDataType,
        cx: &mut Context<Self>,
    ) {
        use std::collections::{HashSet, VecDeque};

        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();
        queue.push_back(start_node_id);

        while let Some(node_id) = queue.pop_front() {
            if visited.contains(&node_id) {
                continue;
            }
            visited.insert(node_id.clone());

            if let Some(node) = self.graph.nodes.iter_mut().find(|n| n.id == node_id) {
                if node.node_type == NodeType::Reroute {
                    // Update pin types
                    for pin in &mut node.inputs {
                        pin.data_type = data_type.clone();
                    }
                    for pin in &mut node.outputs {
                        pin.data_type = data_type.clone();
                    }

                    // Find connected reroute nodes
                    for connection in &self.graph.connections {
                        if connection.source_node == node_id {
                            if let Some(target_node) = self
                                .graph
                                .nodes
                                .iter()
                                .find(|n| n.id == connection.target_node)
                            {
                                if target_node.node_type == NodeType::Reroute {
                                    queue.push_back(connection.target_node.clone());
                                }
                            }
                        } else if connection.target_node == node_id {
                            if let Some(source_node) = self
                                .graph
                                .nodes
                                .iter()
                                .find(|n| n.id == connection.source_node)
                            {
                                if source_node.node_type == NodeType::Reroute {
                                    queue.push_back(connection.source_node.clone());
                                }
                            }
                        }
                    }
                }
            }
        }

        cx.notify();
    }

    /// Get data type of a connection
    pub fn get_connection_data_type(&self, connection: &Connection) -> Option<GraphDataType> {
        let from_node = self
            .graph
            .nodes
            .iter()
            .find(|n| n.id == connection.source_node)?;
        let output_pin = from_node
            .outputs
            .iter()
            .find(|p| p.id == connection.source_pin)?;
        Some(output_pin.data_type.clone())
    }

    /// Find connection near a point (for double-click reroute creation)
    pub fn find_connection_near_point(&self, point: Point<f32>) -> Option<Connection> {
        const CLICK_THRESHOLD: f32 = 30.0;

        for connection in &self.graph.connections {
            let from_node = self
                .graph
                .nodes
                .iter()
                .find(|n| n.id == connection.source_node)?;
            let to_node = self
                .graph
                .nodes
                .iter()
                .find(|n| n.id == connection.target_node)?;

            // Calculate pin positions (simplified - using node edges)
            let from_pos = Point::new(
                from_node.position.x + from_node.size.width,
                from_node.position.y + from_node.size.height / 2.0,
            );
            let to_pos = Point::new(
                to_node.position.x,
                to_node.position.y + to_node.size.height / 2.0,
            );

            // Check if point is near connection line
            if Self::point_near_bezier(point, from_pos, to_pos, CLICK_THRESHOLD) {
                return Some(connection.clone());
            }
        }

        None
    }

    /// Check if point is near a bezier curve (simplified linear approximation)
    fn point_near_bezier(
        point: Point<f32>,
        start: Point<f32>,
        end: Point<f32>,
        threshold: f32,
    ) -> bool {
        // Simplified: check distance to line segment
        let dx = end.x - start.x;
        let dy = end.y - start.y;
        let length_sq = dx * dx + dy * dy;

        if length_sq == 0.0 {
            let dist = ((point.x - start.x).powi(2) + (point.y - start.y).powi(2)).sqrt();
            return dist <= threshold;
        }

        let t = ((point.x - start.x) * dx + (point.y - start.y) * dy) / length_sq;
        let t = t.clamp(0.0, 1.0);

        let closest_x = start.x + t * dx;
        let closest_y = start.y + t * dy;

        let dist = ((point.x - closest_x).powi(2) + (point.y - closest_y).powi(2)).sqrt();
        dist <= threshold
    }
}
