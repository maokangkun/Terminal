use std::path::Path;

use crate::math::{Mat4, Vec3};
use crate::protocols::Rgb;

#[derive(Clone, Copy, Debug)]
pub struct Vertex {
    pub position: Vec3,
    pub normal: Vec3,
    pub uv: [f32; 2],
}

#[derive(Clone, Copy, Debug)]
pub struct Triangle {
    pub vertices: [Vertex; 3],
    pub color: Rgb,
    pub texture: Option<usize>,
}

#[derive(Debug)]
pub struct Texture {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<Rgb>,
}

#[derive(Debug)]
pub struct Model {
    pub triangles: Vec<Triangle>,
    pub textures: Vec<Texture>,
    pub center: Vec3,
    pub radius: f32,
    pub meta: ModelMeta,
}

#[derive(Debug)]
pub struct ModelMeta {
    pub file_name: String,
    pub format: String,
    pub file_size: u64,
    pub scenes: usize,
    pub nodes: usize,
    pub meshes: usize,
    pub materials: usize,
    pub textures: usize,
    pub animations: usize,
    pub primitives: usize,
    pub vertices: usize,
    pub triangles: usize,
    pub radius: f32,
}

#[derive(Default)]
struct LoadStats {
    primitives: usize,
    vertices: usize,
}

pub fn load_glb(path: &Path) -> Result<Model, String> {
    let file_size = std::fs::metadata(path).map(|meta| meta.len()).unwrap_or(0);
    let (document, buffers, images) = gltf::import(path).map_err(|err| err.to_string())?;
    let document_stats = DocumentStats::from_document(&document);
    let textures = images
        .iter()
        .map(texture_from_image)
        .collect::<Result<Vec<_>, _>>()?;
    let mut triangles = Vec::new();
    let mut min = Vec3::splat(f32::INFINITY);
    let mut max = Vec3::splat(f32::NEG_INFINITY);
    let mut stats = LoadStats::default();

    for scene in document.scenes() {
        for node in scene.nodes() {
            collect_node(
                &node,
                Mat4::identity(),
                &buffers,
                &mut triangles,
                &mut min,
                &mut max,
                &mut stats,
            )?;
        }
    }
    if triangles.is_empty() {
        return Err("model has no triangle mesh primitives".into());
    }

    let center = (min + max) * 0.5;
    let radius = (max - center).length().max(0.001);
    let meta = ModelMeta {
        file_name: path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("model")
            .to_string(),
        format: path
            .extension()
            .and_then(|extension| extension.to_str())
            .unwrap_or("glb")
            .to_ascii_lowercase(),
        file_size,
        scenes: document_stats.scenes,
        nodes: document_stats.nodes,
        meshes: document_stats.meshes,
        materials: document_stats.materials,
        textures: textures.len(),
        animations: document_stats.animations,
        primitives: stats.primitives,
        vertices: stats.vertices,
        triangles: triangles.len(),
        radius,
    };
    Ok(Model {
        triangles,
        textures,
        center,
        radius,
        meta,
    })
}

struct DocumentStats {
    scenes: usize,
    nodes: usize,
    meshes: usize,
    materials: usize,
    animations: usize,
}

impl DocumentStats {
    fn from_document(document: &gltf::Document) -> Self {
        Self {
            scenes: document.scenes().count(),
            nodes: document.nodes().count(),
            meshes: document.meshes().count(),
            materials: document.materials().count(),
            animations: document.animations().count(),
        }
    }
}

fn collect_node(
    node: &gltf::Node<'_>,
    parent: Mat4,
    buffers: &[gltf::buffer::Data],
    triangles: &mut Vec<Triangle>,
    min: &mut Vec3,
    max: &mut Vec3,
    stats: &mut LoadStats,
) -> Result<(), String> {
    let transform = parent * Mat4::from_array(node.transform().matrix());
    if let Some(mesh) = node.mesh() {
        for primitive in mesh.primitives() {
            if primitive.mode() != gltf::mesh::Mode::Triangles {
                continue;
            }
            stats.primitives += 1;
            let reader = primitive.reader(|buffer| Some(&buffers[buffer.index()]));
            let positions = reader
                .read_positions()
                .ok_or("mesh primitive is missing positions")?
                .map(Vec3::from_array)
                .collect::<Vec<_>>();
            let normals = reader
                .read_normals()
                .map(|items| items.map(Vec3::from_array).collect::<Vec<_>>());
            let indices = reader
                .read_indices()
                .map(|items| items.into_u32().collect::<Vec<_>>())
                .unwrap_or_else(|| (0..positions.len() as u32).collect());
            stats.vertices += positions.len();
            let texcoords = reader
                .read_tex_coords(0)
                .map(|items| items.into_f32().collect::<Vec<_>>());
            let color = material_color(primitive.material());
            let texture = material_texture(primitive.material());

            for index in indices.chunks_exact(3) {
                let mut verts = [Vertex {
                    position: Vec3::zero(),
                    normal: Vec3::zero(),
                    uv: [0.0, 0.0],
                }; 3];
                for slot in 0..3 {
                    let vertex_index = index[slot] as usize;
                    let position = transform.transform_point(positions[vertex_index]);
                    let normal = normals
                        .as_ref()
                        .and_then(|items| items.get(vertex_index).copied())
                        .map(|n| {
                            transform
                                .transform_vector(n)
                                .normalize_or(Vec3::new(0.0, 1.0, 0.0))
                        })
                        .unwrap_or(Vec3::zero());
                    let uv = texcoords
                        .as_ref()
                        .and_then(|items| items.get(vertex_index).copied())
                        .unwrap_or([0.0, 0.0]);
                    verts[slot] = Vertex {
                        position,
                        normal,
                        uv,
                    };
                    *min = min.min(position);
                    *max = max.max(position);
                }
                if verts[0].normal.length_squared() == 0.0 {
                    let face = (verts[1].position - verts[0].position)
                        .cross(verts[2].position - verts[0].position)
                        .normalize_or(Vec3::new(0.0, 1.0, 0.0));
                    verts[0].normal = face;
                    verts[1].normal = face;
                    verts[2].normal = face;
                }
                triangles.push(Triangle {
                    vertices: verts,
                    color,
                    texture,
                });
            }
        }
    }
    for child in node.children() {
        collect_node(&child, transform, buffers, triangles, min, max, stats)?;
    }
    Ok(())
}

fn material_color(material: gltf::Material<'_>) -> Rgb {
    let factor = material.pbr_metallic_roughness().base_color_factor();
    Rgb {
        r: linear_to_u8(factor[0]),
        g: linear_to_u8(factor[1]),
        b: linear_to_u8(factor[2]),
    }
}

fn material_texture(material: gltf::Material<'_>) -> Option<usize> {
    material
        .pbr_metallic_roughness()
        .base_color_texture()
        .map(|info| info.texture().source().index())
}

fn linear_to_u8(value: f32) -> u8 {
    (value.clamp(0.0, 1.0).powf(1.0 / 2.2) * 255.0).round() as u8
}

fn texture_from_image(image: &gltf::image::Data) -> Result<Texture, String> {
    let width = image.width as usize;
    let height = image.height as usize;
    let pixels = match image.format {
        gltf::image::Format::R8 => image
            .pixels
            .iter()
            .map(|value| Rgb {
                r: *value,
                g: *value,
                b: *value,
            })
            .collect(),
        gltf::image::Format::R8G8 => image
            .pixels
            .chunks_exact(2)
            .map(|chunk| Rgb {
                r: chunk[0],
                g: chunk[1],
                b: 0,
            })
            .collect(),
        gltf::image::Format::R8G8B8 => image
            .pixels
            .chunks_exact(3)
            .map(|chunk| Rgb {
                r: chunk[0],
                g: chunk[1],
                b: chunk[2],
            })
            .collect(),
        gltf::image::Format::R8G8B8A8 => image
            .pixels
            .chunks_exact(4)
            .map(|chunk| Rgb {
                r: chunk[0],
                g: chunk[1],
                b: chunk[2],
            })
            .collect(),
        format => return Err(format!("unsupported texture format: {format:?}")),
    };
    Ok(Texture {
        width,
        height,
        pixels,
    })
}
