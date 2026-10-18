//! Material blueprints from the images that come with an imported model.
//!
//! The editor's import pipeline knows nothing about materials. This module is
//! how the shader plugin tells it "I can help with this": an
//! [`ImportCompanion`] that, when the user accepts the offer, writes one
//! `.material` shader graph per model surface that has images (a base-colour
//! sample wired to the output, and so on), and offers each as the default
//! material of its mesh slot.

use std::path::{Path, PathBuf};

use plugin_editor_api::{
    CompanionOutput, ImportCompanion, ImportCompanionContext, ImportOffer, ImportProbe,
    SourceImageRole, SourceSurface,
};
use psgc::{
    Connection, ConnectionType, DataType, GraphDescription, NodeInstance, Pin, PinInstance,
    PinType, Position,
};

use crate::core::definitions::NodeDefinitions;
use crate::io::formats::{serialize_shader_with_header, ShaderAsset};

/// The id stored with an import, so a re-import runs this companion again.
pub const COMPANION_ID: &str = "pulsar.shader.material_from_images";

const GRAPH_FILE: &str = "shader_graph_save.json";

pub struct MaterialFromImages;

/// The companion to register with the editor's import pipeline.
pub fn material_import_companion() -> std::sync::Arc<dyn ImportCompanion> {
    std::sync::Arc::new(MaterialFromImages)
}

impl ImportCompanion for MaterialFromImages {
    fn id(&self) -> &'static str {
        COMPANION_ID
    }

    fn offer(&self, probe: &ImportProbe) -> Option<ImportOffer> {
        if probe.importer != "mesh" || probe.nearby_images.is_empty() {
            return None;
        }
        let count = probe.nearby_images.len();
        Some(ImportOffer {
            label: "Generate materials from matching textures".into(),
            description: format!(
                "{count} image file{} found next to the model. A material is created for each \
                 surface that has textures, and assigned to the mesh where it has none.",
                if count == 1 { "" } else { "s" }
            ),
            default_enabled: true,
        })
    }

    fn run(&self, context: &ImportCompanionContext) -> Result<Vec<CompanionOutput>, String> {
        let stem = context
            .native_asset
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("model");
        let folder = context
            .native_asset
            .parent()
            .unwrap_or(context.project_root)
            .join(format!("{stem}_materials"));

        let mut outputs = Vec::new();
        let mut used_names = Vec::<String>::new();
        for surface in context.surfaces.iter().filter(|s| !s.images.is_empty()) {
            let name = unique_name(file_safe_name(surface), &used_names);
            used_names.push(name.clone());
            let material = folder.join(format!("{name}.material"));
            // A material that exists may have been edited: keep it, and still
            // report it so the import keeps owning (and assigning) it.
            if !material.join(GRAPH_FILE).exists() {
                write_material(&material, surface, context.project_root)?;
            }
            outputs.push(CompanionOutput {
                path: material,
                kind: "material".into(),
                source_name: Some(surface.name.clone()),
                assign_to_slot: Some(surface.slot),
            });
        }
        Ok(outputs)
    }
}

fn file_safe_name(surface: &SourceSurface) -> String {
    let cleaned: String = surface
        .name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let cleaned = cleaned.trim_matches('_');
    if cleaned.is_empty() {
        format!("Material{}", surface.slot + 1)
    } else {
        cleaned.to_owned()
    }
}

fn unique_name(name: String, used: &[String]) -> String {
    let mut candidate = name.clone();
    let mut n = 2;
    while used.iter().any(|u| u.eq_ignore_ascii_case(&candidate)) {
        candidate = format!("{name}_{n}");
        n += 1;
    }
    candidate
}

fn write_material(
    material: &Path,
    surface: &SourceSurface,
    project_root: &Path,
) -> Result<(), String> {
    let graph = build_graph(surface, |image| texture_reference(image, material, project_root))?;
    let asset = ShaderAsset::from_components(graph, None);
    let text = serialize_shader_with_header(&asset)?;
    std::fs::create_dir_all(material)
        .and_then(|()| std::fs::write(material.join(GRAPH_FILE), text))
        .map_err(|error| format!("write {}: {error}", material.display()))
}

/// How the graph names `image`: project-relative when it is in the project,
/// otherwise a copy beside the material (a material can only sample project
/// files).
fn texture_reference(image: &Path, material: &Path, project_root: &Path) -> Result<String, String> {
    let inside = if image.starts_with(project_root) {
        image.to_path_buf()
    } else {
        let textures = material.with_extension("textures");
        std::fs::create_dir_all(&textures).map_err(|error| error.to_string())?;
        let copy = textures.join(image.file_name().ok_or("image has no file name")?);
        std::fs::copy(image, &copy).map_err(|error| format!("copy {}: {error}", image.display()))?;
        copy
    };
    Ok(inside
        .strip_prefix(project_root)
        .unwrap_or(&inside)
        .to_string_lossy()
        .replace('\\', "/"))
}

fn node(id: &str, kind: &str, x: f64, y: f64) -> Result<NodeInstance, String> {
    let definition = NodeDefinitions::load()
        .get_node_definition(kind)
        .ok_or_else(|| format!("shader node '{kind}' is not available"))?;
    let pin_type = |pin: &crate::core::definitions::PinDefinition| {
        if pin.data_type.is_execution() {
            DataType::Exec
        } else {
            DataType::typed(pin.data_type.type_name.clone())
        }
    };
    let mut instance = NodeInstance::new(id, kind, Position { x, y });
    for input in &definition.inputs {
        instance.inputs.push(PinInstance::new(
            input.id.clone(),
            Pin::new(input.id.clone(), input.name.clone(), pin_type(input), PinType::Input),
        ));
    }
    for output in &definition.outputs {
        instance.outputs.push(PinInstance::new(
            output.id.clone(),
            Pin::new(output.id.clone(), output.name.clone(), pin_type(output), PinType::Output),
        ));
    }
    Ok(instance)
}

fn connect(graph: &mut GraphDescription, from: &str, to: &str, pin: &str) {
    graph.add_connection(Connection::new(from, "result", to, pin, ConnectionType::Data));
}

/// One image sample per image, wired to the output pin its role feeds. A
/// normal map is sampled but left unwired: the output's normal pin is
/// world-space and the tangent-space decode is the artist's call.
fn build_graph(
    surface: &SourceSurface,
    mut reference: impl FnMut(&Path) -> Result<String, String>,
) -> Result<GraphDescription, String> {
    let mut graph = GraphDescription::new(&surface.name);
    graph.add_node(node("output", "fragment_output", 900.0, 200.0)?);
    graph.add_node(node("uv", "frag_uv", 0.0, 200.0)?);

    for (row, image) in surface.images.iter().enumerate() {
        let y = row as f64 * 220.0;
        let sample = format!("sample_{row}");
        let mut sampler = node(&sample, "sample_texture", 300.0, y)?;
        sampler
            .properties
            .insert("texture".into(), serde_json::json!(reference(&image.path)?));
        graph.add_node(sampler);
        connect(&mut graph, "uv", &sample, "uv");

        // (channel node, output pin) pairs fed by this image.
        let feeds: &[(Option<&str>, &str)] = match image.role {
            SourceImageRole::BaseColor => &[(None, "base_color")],
            SourceImageRole::Emissive => &[(None, "emissive_color")],
            SourceImageRole::MetallicRoughness => {
                &[(Some("component_g"), "roughness"), (Some("component_b"), "metallic")]
            }
            SourceImageRole::Occlusion => &[(Some("component_r"), "ambient_occlusion")],
            SourceImageRole::Normal => &[],
        };
        for (channel, pin) in feeds {
            match channel {
                None => connect(&mut graph, &sample, "output", pin),
                Some(kind) => {
                    let id = format!("{kind}_{row}");
                    graph.add_node(node(&id, kind, 600.0, y)?);
                    connect(&mut graph, &sample, &id, "vec");
                    connect(&mut graph, &id, "output", pin);
                }
            }
        }
    }
    Ok(graph)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::formats::deserialize_shader;
    use plugin_editor_api::SourceImage;

    fn surface(roles: &[(SourceImageRole, &str)]) -> SourceSurface {
        SourceSurface {
            slot: 1,
            name: "Rock Wall".into(),
            images: roles
                .iter()
                .map(|(role, file)| SourceImage {
                    role: *role,
                    path: PathBuf::from(file),
                })
                .collect(),
        }
    }

    #[test]
    fn roles_feed_their_output_pins() {
        let graph = build_graph(
            &surface(&[
                (SourceImageRole::BaseColor, "a.png"),
                (SourceImageRole::MetallicRoughness, "mr.png"),
                (SourceImageRole::Normal, "n.png"),
            ]),
            |path| Ok(path.to_string_lossy().into_owned()),
        )
        .unwrap();
        let into_output = |pin: &str| {
            graph
                .connections
                .iter()
                .any(|c| c.target_node == "output" && c.target_pin == pin)
        };
        assert!(into_output("base_color") && into_output("roughness") && into_output("metallic"));
        // The normal map is sampled but not wired.
        assert!(graph.nodes.contains_key("sample_2"));
        assert!(!graph.connections.iter().any(|c| c.source_node == "sample_2" && c.target_node == "output"));
        assert_eq!(
            graph.nodes["sample_1"].properties["texture"],
            serde_json::json!("mr.png")
        );
    }

    #[test]
    fn run_writes_a_loadable_material_once_and_keeps_edits() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("rock.png"), b"png").unwrap();
        let native = root.join("rock.mesh");
        let surfaces = vec![
            surface(&[(SourceImageRole::BaseColor, root.join("rock.png").to_str().unwrap())]),
            SourceSurface { slot: 0, name: "Bare".into(), images: vec![] },
        ];
        let context = ImportCompanionContext {
            project_root: root,
            source: &root.join("rock.fbx"),
            native_asset: &native,
            surfaces: &surfaces,
        };
        let outputs = MaterialFromImages.run(&context).unwrap();
        assert_eq!(outputs.len(), 1, "only surfaces with images get a material");
        assert_eq!(outputs[0].assign_to_slot, Some(1));
        let graph_file = outputs[0].path.join(GRAPH_FILE);
        let written = std::fs::read_to_string(&graph_file).unwrap();
        let asset = deserialize_shader(&written).unwrap();
        assert_eq!(
            asset.main_graph.nodes["sample_0"].properties["texture"],
            serde_json::json!("rock.png")
        );

        std::fs::write(&graph_file, "edited").unwrap();
        MaterialFromImages.run(&context).unwrap();
        assert_eq!(std::fs::read_to_string(&graph_file).unwrap(), "edited");
    }

    #[test]
    fn offer_needs_the_mesh_importer_and_nearby_images() {
        let sources = [PathBuf::from("a.fbx")];
        let probe = |importer, nearby: &[PathBuf]| {
            MaterialFromImages
                .offer(&ImportProbe { importer, sources: &sources, nearby_images: nearby })
                .is_some()
        };
        assert!(probe("mesh", &[PathBuf::from("x.png")]));
        assert!(!probe("mesh", &[]));
        assert!(!probe("other", &[PathBuf::from("x.png")]));
    }
}
