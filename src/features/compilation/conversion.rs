//! Graph conversion - Convert between BlueprintGraph and psgc::GraphDescription

use crate::editor::panel::ShaderEditorPanel;
use crate::rendering::layout;
use crate::{
    BlueprintComment, BlueprintGraph, BlueprintNode, Connection, NodeDefinitions, NodeType, Pin,
    PinType,
};
use gpui::*;
use psgc::{
    compile_fragment_shader, Connection as PsgcConnection, ConnectionType as PsgcConnectionType,
    GraphDescription, NodeInstance, Pin as PsgcPin, PinInstance, PinType as PsgcPinType, Position,
    PropertyValue, TypeInfo,
};

/// Convert our internal `PinDataType` (a free-form type-name string) into the
/// psgc/graphy `DataType` representation used by the compiler.
fn pin_data_type_to_psgc(data_type: &crate::core::types::PinDataType) -> psgc::DataType {
    if data_type.is_execution() {
        psgc::DataType::Execution
    } else {
        psgc::DataType::Typed(TypeInfo::new(data_type.type_name.clone()))
    }
}

/// Convert a psgc/graphy `DataType` back into our internal `PinDataType`.
fn psgc_data_type_to_pin_data_type(data_type: &psgc::DataType) -> crate::core::types::PinDataType {
    match data_type {
        psgc::DataType::Execution => crate::core::types::PinDataType::execution(),
        psgc::DataType::Typed(info) => {
            crate::core::types::PinDataType::from_type_str(info.type_string.clone())
        }
        psgc::DataType::Number => crate::core::types::PinDataType::from_type_str("f64"),
        psgc::DataType::String => crate::core::types::PinDataType::from_type_str("String"),
        psgc::DataType::Boolean => crate::core::types::PinDataType::from_type_str("bool"),
        psgc::DataType::Vector2 => crate::core::types::PinDataType::from_type_str("Vec2"),
        psgc::DataType::Vector3 => crate::core::types::PinDataType::from_type_str("Vec3"),
        psgc::DataType::Color => crate::core::types::PinDataType::from_type_str("Color"),
        psgc::DataType::Any => crate::core::types::PinDataType::wildcard(),
    }
}

impl ShaderEditorPanel {
    /// Convert current blueprint graph to psgc GraphDescription
    pub(crate) fn convert_to_graph_description(
        &self,
        graph: &crate::core::graph::BlueprintGraph,
    ) -> Result<GraphDescription, String> {
        self.convert_graph_to_description(graph)
    }

    /// Convert any blueprint graph to psgc GraphDescription
    pub(crate) fn convert_graph_to_description(
        &self,
        graph: &BlueprintGraph,
    ) -> Result<GraphDescription, String> {
        let mut graph_desc = GraphDescription::new("Shader Graph");

        // Convert nodes
        for bp_node in &graph.nodes {
            let node_type = bp_node.definition_id.clone();
            let mut node_instance = NodeInstance::new(
                &bp_node.id,
                &node_type,
                Position {
                    x: bp_node.position.x as f64,
                    y: bp_node.position.y as f64,
                },
            );

            // Convert input pins
            for pin in &bp_node.inputs {
                node_instance.inputs.push(PinInstance {
                    id: pin.id.clone(),
                    pin: PsgcPin {
                        id: pin.id.clone(),
                        name: pin.name.clone(),
                        pin_type: PsgcPinType::Input,
                        data_type: pin_data_type_to_psgc(&pin.data_type),
                    },
                });
            }

            // Convert output pins
            for pin in &bp_node.outputs {
                node_instance.outputs.push(PinInstance {
                    id: pin.id.clone(),
                    pin: PsgcPin {
                        id: pin.id.clone(),
                        name: pin.name.clone(),
                        pin_type: PsgcPinType::Output,
                        data_type: pin_data_type_to_psgc(&pin.data_type),
                    },
                });
            }

            // Convert properties
            for (key, value) in &bp_node.properties {
                let prop_value = if let Ok(n) = value.parse::<f64>() {
                    PropertyValue::Number(n)
                } else if let Ok(b) = value.parse::<bool>() {
                    PropertyValue::Boolean(b)
                } else {
                    PropertyValue::String(value.clone())
                };
                node_instance.set_property(key, prop_value);
            }

            graph_desc.add_node(node_instance);
        }

        // Convert connections
        for connection in &graph.connections {
            let conn_type = graph
                .nodes
                .iter()
                .find(|n| n.id == connection.source_node)
                .and_then(|node| node.outputs.iter().find(|p| p.id == connection.source_pin))
                .map(|pin| {
                    if pin.data_type.is_execution() {
                        PsgcConnectionType::Execution
                    } else {
                        PsgcConnectionType::Data
                    }
                })
                .unwrap_or(PsgcConnectionType::Data);

            let psgc_connection = PsgcConnection::new(
                &connection.source_node,
                &connection.source_pin,
                &connection.target_node,
                &connection.target_pin,
                conn_type,
            );
            graph_desc.add_connection(psgc_connection);
        }

        // Convert comments
        graph_desc.comments = graph
            .comments
            .iter()
            .map(|c| graphy::core::GraphComment {
                text: c.text.clone(),
                position: Position {
                    x: c.position.x as f64,
                    y: c.position.y as f64,
                },
                size: (c.size.width as f64, c.size.height as f64),
            })
            .collect();

        Ok(graph_desc)
    }

    /// Get node type from blueprint node
    fn get_node_type_from_blueprint(&self, bp_node: &BlueprintNode) -> Result<String, String> {
        Ok(bp_node.definition_id.clone())
    }

    /// Convert psgc GraphDescription back to BlueprintGraph
    pub(crate) fn convert_graph_description_to_blueprint(
        &mut self,
        graph_desc: &GraphDescription,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<BlueprintGraph, String> {
        let mut nodes = Vec::new();
        let mut connections = Vec::new();

        let node_definitions = NodeDefinitions::load();

        // Convert nodes
        for (node_id, node_instance) in &graph_desc.nodes {
            let definition_id = node_instance.node_type.clone();
            let node_def = node_definitions.get_node_definition(&definition_id);

            let (title, icon, description, node_type, color) = if definition_id == "reroute" {
                (
                    "Reroute".to_string(),
                    "•".to_string(),
                    "Reroute node for organizing connections".to_string(),
                    NodeType::Reroute,
                    None,
                )
            } else if let Some(def) = node_def {
                let ntype = if def.is_event {
                    NodeType::Event
                } else {
                    let category = node_definitions.get_category_for_node(&def.id);
                    match category.map(|c| c.name.as_str()) {
                        Some("Logic") => NodeType::Logic,
                        Some("Math") => NodeType::Math,
                        Some("Object") => NodeType::Object,
                        _ => NodeType::Logic,
                    }
                };
                (
                    def.name.clone(),
                    def.icon.clone(),
                    def.description.clone(),
                    ntype,
                    def.color.clone(),
                )
            } else {
                (
                    definition_id.replace('_', " "),
                    "⚙".to_string(),
                    String::new(),
                    NodeType::Logic,
                    None,
                )
            };

            let bp_node = BlueprintNode {
                id: node_id.clone(),
                definition_id,
                title,
                icon,
                node_type,
                position: Point::new(
                    node_instance.position.x as f32,
                    node_instance.position.y as f32,
                ),
                size: crate::Size::new(
                    layout::node_width_for_pins(
                        &node_instance
                            .outputs
                            .iter()
                            .map(|pin_inst| {
                                let pin = &pin_inst.pin;
                                Pin {
                                    id: pin_inst.id.clone(),
                                    name: pin.name.clone(),
                                    pin_type: match pin.pin_type {
                                        PsgcPinType::Input => PinType::Input,
                                        PsgcPinType::Output => PinType::Output,
                                    },
                                    data_type: psgc_data_type_to_pin_data_type(&pin.data_type),
                                }
                            })
                            .collect::<Vec<_>>(),
                    ),
                    layout::node_height_for_pins(
                        &node_instance
                            .inputs
                            .iter()
                            .map(|pin_inst| {
                                let pin = &pin_inst.pin;
                                Pin {
                                    id: pin_inst.id.clone(),
                                    name: pin.name.clone(),
                                    pin_type: match pin.pin_type {
                                        PsgcPinType::Input => PinType::Input,
                                        PsgcPinType::Output => PinType::Output,
                                    },
                                    data_type: psgc_data_type_to_pin_data_type(&pin.data_type),
                                }
                            })
                            .collect::<Vec<_>>(),
                        &node_instance
                            .outputs
                            .iter()
                            .map(|pin_inst| {
                                let pin = &pin_inst.pin;
                                Pin {
                                    id: pin_inst.id.clone(),
                                    name: pin.name.clone(),
                                    pin_type: match pin.pin_type {
                                        PsgcPinType::Input => PinType::Input,
                                        PsgcPinType::Output => PinType::Output,
                                    },
                                    data_type: psgc_data_type_to_pin_data_type(&pin.data_type),
                                }
                            })
                            .collect::<Vec<_>>(),
                    ),
                ),
                inputs: node_instance
                    .inputs
                    .iter()
                    .map(|pin_inst| {
                        let pin = &pin_inst.pin;
                        Pin {
                            id: pin_inst.id.clone(),
                            name: pin.name.clone(),
                            pin_type: match pin.pin_type {
                                PsgcPinType::Input => PinType::Input,
                                PsgcPinType::Output => PinType::Output,
                            },
                            data_type: psgc_data_type_to_pin_data_type(&pin.data_type),
                        }
                    })
                    .collect(),
                outputs: node_instance
                    .outputs
                    .iter()
                    .map(|pin_inst| {
                        let pin = &pin_inst.pin;
                        Pin {
                            id: pin_inst.id.clone(),
                            name: pin.name.clone(),
                            pin_type: match pin.pin_type {
                                PsgcPinType::Input => PinType::Input,
                                PsgcPinType::Output => PinType::Output,
                            },
                            data_type: psgc_data_type_to_pin_data_type(&pin.data_type),
                        }
                    })
                    .collect(),
                properties: node_instance
                    .properties
                    .iter()
                    .map(|(k, v)| {
                        let value_str = match v {
                            PropertyValue::String(s) => s.clone(),
                            PropertyValue::Number(n) => n.to_string(),
                            PropertyValue::Boolean(b) => b.to_string(),
                            PropertyValue::Vector2(x, y) => format!("{},{}", x, y),
                            PropertyValue::Vector3(x, y, z) => format!("{},{},{}", x, y, z),
                            PropertyValue::Color(r, g, b, a) => format!("{},{},{},{}", r, g, b, a),
                        };
                        (k.clone(), value_str)
                    })
                    .collect(),
                is_selected: false,
                description,
                color,
            };
            nodes.push(bp_node);
        }

        // Convert connections
        for connection in &graph_desc.connections {
            let bp_connection = Connection {
                id: format!(
                    "{}:{}->{}:{}",
                    connection.source_node,
                    connection.source_pin,
                    connection.target_node,
                    connection.target_pin
                ),
                source_node: connection.source_node.clone(),
                source_pin: connection.source_pin.clone(),
                target_node: connection.target_node.clone(),
                target_pin: connection.target_pin.clone(),
                connection_type: match connection.connection_type {
                    PsgcConnectionType::Execution => ui::graph::ConnectionType::Execution,
                    PsgcConnectionType::Data => ui::graph::ConnectionType::Data,
                },
            };
            connections.push(bp_connection);
        }

        // Convert comments with initialized color picker states
        let comments: Vec<BlueprintComment> = graph_desc
            .comments
            .iter()
            .map(|c| {
                let color = Hsla {
                    h: 0.5,
                    s: 0.3,
                    l: 0.2,
                    a: 0.3,
                };
                let color_picker_state =
                    Some(cx.new(|cx| ui::color_picker::ColorPickerState::new(window, cx)));

                BlueprintComment {
                    id: uuid::Uuid::new_v4().to_string(),
                    text: c.text.clone(),
                    position: Point::new(c.position.x as f32, c.position.y as f32),
                    size: crate::Size::new(c.size.0 as f32, c.size.1 as f32),
                    color,
                    contained_node_ids: Vec::new(),
                    is_selected: false,
                    color_picker_state,
                }
            })
            .collect();

        Ok(BlueprintGraph {
            nodes,
            connections,
            comments,
            selected_nodes: Vec::new(),
            selected_comments: Vec::new(),
            zoom_level: 1.0,
            pan_offset: Point::new(0.0, 0.0),
            virtualization_stats: crate::VirtualizationStats::default(),
        })
    }

    pub(crate) fn compile_preview_wgsl_for_pin(
        &self,
        graph: &BlueprintGraph,
        node_id: &str,
        pin_id: &str,
    ) -> Result<String, String> {
        let node = graph
            .nodes
            .iter()
            .find(|node| node.id == node_id)
            .ok_or_else(|| format!("Preview node '{}' not found", node_id))?;
        let pin = node
            .outputs
            .iter()
            .find(|pin| pin.id == pin_id)
            .ok_or_else(|| format!("Preview pin '{}' not found on node '{}'", pin_id, node_id))?;

        if !pin.data_type.is_texture_previewable() {
            return Err(format!(
                "Pin '{}.{}' is not texture-previewable ({})",
                node_id, pin_id, pin.data_type
            ));
        }

        let mut graph_desc = self.convert_graph_to_description(graph)?;
        graph_desc.nodes.retain(|_, node| {
            node.node_type != "fragment_output" && node.node_type != "vertex_output"
        });
        graph_desc.connections.retain(|connection| {
            graph_desc.nodes.contains_key(&connection.source_node)
                && graph_desc.nodes.contains_key(&connection.target_node)
        });

        let fragment_output = NodeDefinitions::load()
            .get_node_definition("fragment_output")
            .ok_or_else(|| "Missing fragment_output definition".to_string())?;
        let color_input = fragment_output
            .inputs
            .iter()
            .find(|input| input.id == "color")
            .or_else(|| fragment_output.inputs.first())
            .ok_or_else(|| "fragment_output has no inputs".to_string())?;

        let preview_output_id = "__pin_preview_output__".to_string();
        let mut output_node = NodeInstance::new(
            preview_output_id.clone(),
            "fragment_output",
            Position { x: 0.0, y: 0.0 },
        );
        for input in &fragment_output.inputs {
            output_node.inputs.push(PinInstance {
                id: input.id.clone(),
                pin: PsgcPin {
                    id: input.id.clone(),
                    name: input.name.clone(),
                    pin_type: PsgcPinType::Input,
                    data_type: pin_data_type_to_psgc(&input.data_type),
                },
            });
        }
        graph_desc.add_node(output_node);
        graph_desc.add_connection(PsgcConnection::new(
            node_id,
            pin_id,
            &preview_output_id,
            &color_input.id,
            PsgcConnectionType::Data,
        ));

        let mut wgsl = compile_fragment_shader(&graph_desc)
            .map_err(|e| format!("Preview WGSL compilation failed: {}", e))?;
        if matches!(pin.data_type.type_name.as_str(), "vec3<f32>") {
            wgsl = wrap_vec3_preview_return(&wgsl)?;
        }

        Ok(wgsl)
    }
}

fn wrap_vec3_preview_return(wgsl: &str) -> Result<String, String> {
    let mut replaced = false;
    let mut lines = Vec::new();

    for line in wgsl.lines() {
        let trimmed = line.trim_start();
        if !replaced && trimmed.starts_with("return ") && trimmed.ends_with(';') {
            let indent_len = line.len() - trimmed.len();
            let indent = &line[..indent_len];
            let expr = trimmed
                .trim_start_matches("return ")
                .trim_end_matches(';')
                .trim();
            lines.push(format!("{}return vec4<f32>({}, 1.0);", indent, expr));
            replaced = true;
        } else {
            lines.push(line.to_string());
        }
    }

    if replaced {
        Ok(lines.join("\n"))
    } else {
        Err("Unable to adapt vec3 preview shader output".to_string())
    }
}
