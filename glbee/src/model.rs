use std::collections::HashMap;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use flate2::read::ZlibDecoder;

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

pub fn load_model(path: &Path) -> Result<Model, String> {
    match model_format(path).as_str() {
        "glb" | "gltf" => load_glb(path),
        "obj" => load_obj(path),
        "fbx" => load_fbx(path),
        "3ds" => load_3ds(path),
        "blend" => load_blend(path),
        "usdz" => load_usdz(path),
        format => Err(format!(
            "unsupported model format: {format} (supported: glb, gltf, obj, fbx, 3ds, blend, usdz)"
        )),
    }
}

fn model_format(path: &Path) -> String {
    path.extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
}

fn load_glb(path: &Path) -> Result<Model, String> {
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

fn load_obj(path: &Path) -> Result<Model, String> {
    let text = std::fs::read_to_string(path).map_err(|err| err.to_string())?;
    let file_size = std::fs::metadata(path).map(|meta| meta.len()).unwrap_or(0);
    let mut positions = Vec::<Vec3>::new();
    let mut normals = Vec::<Vec3>::new();
    let mut texcoords = Vec::<[f32; 2]>::new();
    let mut triangles = Vec::<Triangle>::new();
    let mut textures = Vec::<Texture>::new();
    let materials = load_obj_materials(path, &text, &mut textures)?;
    let fallback_material = (materials.len() == 1)
        .then(|| materials.values().next().copied())
        .flatten()
        .unwrap_or_default();
    let mut current_material = fallback_material;

    for raw_line in text.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.split_whitespace();
        match parts.next() {
            Some("v") => {
                let x = parse_f32(parts.next(), "obj vertex x")?;
                let y = parse_f32(parts.next(), "obj vertex y")?;
                let z = parse_f32(parts.next(), "obj vertex z")?;
                positions.push(Vec3::new(x, y, z));
            }
            Some("vn") => {
                let x = parse_f32(parts.next(), "obj normal x")?;
                let y = parse_f32(parts.next(), "obj normal y")?;
                let z = parse_f32(parts.next(), "obj normal z")?;
                normals.push(Vec3::new(x, y, z).normalize_or(Vec3::new(0.0, 1.0, 0.0)));
            }
            Some("vt") => {
                let u = parse_f32(parts.next(), "obj texcoord u")?;
                let v = parse_f32(parts.next(), "obj texcoord v")?;
                texcoords.push([u, v]);
            }
            Some("usemtl") => {
                if let Some(name) = parts.next() {
                    current_material = materials.get(name).copied().unwrap_or(fallback_material);
                }
            }
            Some("f") => {
                let face = parts
                    .map(|item| {
                        parse_obj_face_vertex(item, positions.len(), texcoords.len(), normals.len())
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                if face.len() < 3 {
                    continue;
                }
                for i in 1..face.len() - 1 {
                    let refs = [face[0], face[i], face[i + 1]];
                    let mut verts = [Vertex {
                        position: Vec3::zero(),
                        normal: Vec3::zero(),
                        uv: [0.0, 0.0],
                    }; 3];
                    for (slot, reference) in refs.iter().enumerate() {
                        verts[slot].position = positions[reference.position];
                        verts[slot].uv = reference
                            .texcoord
                            .and_then(|index| texcoords.get(index).copied())
                            .unwrap_or([0.0, 0.0]);
                        verts[slot].normal = reference
                            .normal
                            .and_then(|index| normals.get(index).copied())
                            .unwrap_or(Vec3::zero());
                    }
                    fill_missing_normals(&mut verts);
                    triangles.push(Triangle {
                        vertices: verts,
                        color: current_material.color,
                        texture: current_material.texture,
                    });
                }
            }
            _ => {}
        }
    }

    let texture_count = textures.len();
    build_model(
        path,
        "obj",
        file_size,
        triangles,
        textures,
        ModelCounts {
            scenes: 1,
            nodes: 1,
            meshes: 1,
            materials: materials.len(),
            textures: texture_count,
            animations: 0,
            primitives: 1,
            vertices: positions.len(),
        },
    )
}

#[derive(Clone, Copy)]
struct ObjMaterial {
    color: Rgb,
    texture: Option<usize>,
}

impl Default for ObjMaterial {
    fn default() -> Self {
        Self {
            color: default_color(),
            texture: None,
        }
    }
}

#[derive(Clone, Copy)]
struct ObjFaceVertex {
    position: usize,
    texcoord: Option<usize>,
    normal: Option<usize>,
}

fn parse_obj_face_vertex(
    raw: &str,
    positions: usize,
    texcoords: usize,
    normals: usize,
) -> Result<ObjFaceVertex, String> {
    let mut parts = raw.split('/');
    let position = parse_obj_index(parts.next(), positions, "obj face position")?
        .ok_or("obj face is missing position index")?;
    let texcoord = parse_obj_index(parts.next(), texcoords, "obj face texcoord")?;
    let normal = parse_obj_index(parts.next(), normals, "obj face normal")?;
    Ok(ObjFaceVertex {
        position,
        texcoord,
        normal,
    })
}

fn parse_obj_index(raw: Option<&str>, len: usize, label: &str) -> Result<Option<usize>, String> {
    let Some(raw) = raw else {
        return Ok(None);
    };
    if raw.is_empty() {
        return Ok(None);
    }
    let value = raw
        .parse::<isize>()
        .map_err(|_| format!("{label} must be an integer"))?;
    let index = if value < 0 {
        len as isize + value
    } else {
        value - 1
    };
    if index < 0 || index as usize >= len {
        return Err(format!("{label} index out of range: {raw}"));
    }
    Ok(Some(index as usize))
}

fn parse_f32(raw: Option<&str>, label: &str) -> Result<f32, String> {
    raw.ok_or_else(|| format!("{label} is missing"))?
        .parse::<f32>()
        .map_err(|_| format!("{label} must be a number"))
}

fn load_obj_materials(
    obj_path: &Path,
    obj_text: &str,
    textures: &mut Vec<Texture>,
) -> Result<HashMap<String, ObjMaterial>, String> {
    let mut materials = HashMap::<String, ObjMaterial>::new();
    for raw_line in obj_text.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.split_whitespace();
        if parts.next() == Some("mtllib") {
            let Some(raw_path) = parts.next() else {
                continue;
            };
            let mtl_path = resolve_sibling_path(obj_path, raw_path);
            if !mtl_path.is_file() {
                continue;
            }
            read_mtl_file(&mtl_path, textures, &mut materials)?;
        }
    }
    Ok(materials)
}

fn read_mtl_file(
    mtl_path: &Path,
    textures: &mut Vec<Texture>,
    materials: &mut HashMap<String, ObjMaterial>,
) -> Result<(), String> {
    let text = std::fs::read_to_string(mtl_path).map_err(|err| err.to_string())?;
    let mut current_name = None::<String>;
    let mut current = ObjMaterial::default();
    for raw_line in text.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.split_whitespace();
        match parts.next() {
            Some("newmtl") => {
                if let Some(name) = current_name.take() {
                    materials.insert(name, current);
                }
                current_name = parts.next().map(str::to_string);
                current = ObjMaterial::default();
            }
            Some("Kd") => {
                let r = parse_f32(parts.next(), "mtl diffuse r")?;
                let g = parse_f32(parts.next(), "mtl diffuse g")?;
                let b = parse_f32(parts.next(), "mtl diffuse b")?;
                current.color = Rgb {
                    r: linear_to_u8(r),
                    g: linear_to_u8(g),
                    b: linear_to_u8(b),
                };
            }
            Some("map_Kd") => {
                if let Some(raw_texture) = mtl_texture_path(line) {
                    let texture_path = resolve_sibling_path(mtl_path, raw_texture);
                    if let Some(texture) = decode_texture_file(&texture_path)? {
                        let index = textures.len();
                        textures.push(texture);
                        current.texture = Some(index);
                    }
                }
            }
            _ => {}
        }
    }
    if let Some(name) = current_name {
        materials.insert(name, current);
    }
    Ok(())
}

fn mtl_texture_path(line: &str) -> Option<&str> {
    line.split_whitespace().last()
}

fn resolve_sibling_path(base: &Path, raw: &str) -> PathBuf {
    let path = Path::new(raw);
    if path.is_absolute() {
        return path.to_path_buf();
    }
    base.parent().unwrap_or_else(|| Path::new(".")).join(path)
}

fn load_blend(path: &Path) -> Result<Model, String> {
    if let Ok(mut model) = load_blend_with_blender(path) {
        tag_blend_meta(path, &mut model, "blend");
        return Ok(model);
    }
    for extension in ["obj", "3ds", "fbx", "glb", "gltf"] {
        let fallback = path.with_extension(extension);
        if fallback.is_file() {
            let mut model = load_model(&fallback)?;
            tag_blend_meta(path, &mut model, &format!("blend->{extension}"));
            return Ok(model);
        }
    }
    Err("cannot load .blend: install Blender CLI or place an exported same-name .obj/.3ds/.fbx/.glb next to it".into())
}

fn load_blend_with_blender(path: &Path) -> Result<Model, String> {
    let blender = find_blender_command().ok_or("blender command not found")?;
    let temp_obj = std::env::temp_dir().join(format!(
        "glbee-blend-{}-{}.obj",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|err| err.to_string())?
            .as_nanos()
    ));
    let script = format!(
        "import bpy\n\
         p={}\n\
         try:\n\
             bpy.ops.object.select_all(action='SELECT')\n\
         except Exception:\n\
             pass\n\
         try:\n\
             bpy.ops.wm.obj_export(filepath=p, export_selected_objects=False)\n\
         except Exception:\n\
             bpy.ops.export_scene.obj(filepath=p)\n",
        python_string_literal(&temp_obj)
    );
    let output = Command::new(blender)
        .arg("-b")
        .arg(path)
        .arg("--python-expr")
        .arg(script)
        .output()
        .map_err(|err| err.to_string())?;
    if !output.status.success() || !temp_obj.is_file() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    load_obj(&temp_obj)
}

fn find_blender_command() -> Option<&'static str> {
    for candidate in [
        "blender",
        "/Applications/Blender.app/Contents/MacOS/Blender",
    ] {
        if Command::new(candidate).arg("--version").output().is_ok() {
            return Some(candidate);
        }
    }
    None
}

fn python_string_literal(path: &Path) -> String {
    let raw = path.to_string_lossy();
    let escaped = raw.replace('\\', "\\\\").replace('\'', "\\'");
    format!("'{escaped}'")
}

fn tag_blend_meta(path: &Path, model: &mut Model, format: &str) {
    model.meta.file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("model.blend")
        .to_string();
    model.meta.format = format.to_string();
}

fn load_usdz(path: &Path) -> Result<Model, String> {
    let file_size = std::fs::metadata(path).map(|meta| meta.len()).unwrap_or(0);
    let usda = usdcat_text(path)?;
    let points = parse_vec3_array(&usda, "point3f[] points =")
        .or_else(|| parse_vec3_array(&usda, "point3d[] points ="))
        .ok_or("USDZ mesh is missing points")?;
    let face_counts = parse_usd_int_array(&usda, "int[] faceVertexCounts =")
        .ok_or("USDZ mesh is missing faceVertexCounts")?;
    let face_indices = parse_usd_int_array(&usda, "int[] faceVertexIndices =")
        .ok_or("USDZ mesh is missing faceVertexIndices")?;
    let uvs = parse_uv_array(&usda, "texCoord2f[] primvars:st0 =")
        .or_else(|| parse_uv_array(&usda, "texCoord2f[] primvars:st ="))
        .unwrap_or_default();
    let textures = load_usdz_texture(path, &usda)?
        .map(|texture| vec![texture])
        .unwrap_or_default();
    let texture = (!textures.is_empty()).then_some(0usize);
    let color = if texture.is_some() {
        Rgb {
            r: 255,
            g: 255,
            b: 255,
        }
    } else {
        default_color()
    };
    let mut triangles = Vec::new();
    let mut polygon_vertex = 0usize;

    for count in face_counts {
        if count < 3 {
            polygon_vertex += count;
            continue;
        }
        let face = face_indices
            .get(polygon_vertex..polygon_vertex + count)
            .ok_or("USDZ face index buffer is truncated")?;
        for slot in 1..count - 1 {
            let corners = [0usize, slot, slot + 1];
            let mut vertices = [Vertex {
                position: Vec3::zero(),
                normal: Vec3::zero(),
                uv: [0.0, 0.0],
            }; 3];
            for target in 0..3 {
                let corner = corners[target];
                let point_index = face[corner].max(0) as usize;
                let position = points
                    .get(point_index)
                    .copied()
                    .ok_or("USDZ face references a missing point")?;
                vertices[target] = Vertex {
                    position,
                    normal: Vec3::zero(),
                    uv: usdz_uv(
                        &uvs,
                        points.len(),
                        face_indices.len(),
                        point_index,
                        polygon_vertex + corner,
                    ),
                };
            }
            fill_missing_normals(&mut vertices);
            triangles.push(Triangle {
                vertices,
                color,
                texture,
            });
        }
        polygon_vertex += count;
    }

    let texture_count = textures.len();
    build_model(
        path,
        "usdz",
        file_size,
        triangles,
        textures,
        ModelCounts {
            scenes: 1,
            nodes: 0,
            meshes: 1,
            materials: texture_count,
            textures: texture_count,
            animations: 0,
            primitives: 1,
            vertices: points.len(),
        },
    )
}

fn usdcat_text(path: &Path) -> Result<String, String> {
    let output = Command::new("usdcat")
        .arg(path)
        .output()
        .map_err(|err| format!("cannot run usdcat for USDZ support: {err}"))?;
    if !output.status.success() {
        return Err(format!(
            "usdcat failed for {}: {}",
            path.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    String::from_utf8(output.stdout).map_err(|err| format!("usdcat emitted non-UTF8 text: {err}"))
}

fn load_usdz_texture(path: &Path, usda: &str) -> Result<Option<Texture>, String> {
    let Some(asset_path) = parse_usd_asset_path(usda) else {
        return Ok(None);
    };
    let extension = Path::new(&asset_path)
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or("texture");
    let tmp_path = std::env::temp_dir().join(format!(
        "glbee-usdz-{}-{}.{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0),
        extension
    ));
    let output = Command::new("unzip")
        .arg("-p")
        .arg(path)
        .arg(&asset_path)
        .output()
        .map_err(|err| format!("cannot extract USDZ texture {asset_path}: {err}"))?;
    if !output.status.success() || output.stdout.is_empty() {
        return Ok(None);
    }
    std::fs::write(&tmp_path, &output.stdout).map_err(|err| err.to_string())?;
    let texture = decode_texture_file(&tmp_path)?;
    let _ = std::fs::remove_file(&tmp_path);
    Ok(texture)
}

fn parse_usd_asset_path(text: &str) -> Option<String> {
    let marker = "asset inputs:file = @";
    let start = text.find(marker)? + marker.len();
    let rest = text.get(start..)?;
    let end = rest.find('@')?;
    Some(rest[..end].to_string())
}

fn parse_vec3_array(text: &str, marker: &str) -> Option<Vec<Vec3>> {
    let array = extract_usd_array(text, marker)?;
    Some(
        parse_usd_float_tuples(array)
            .into_iter()
            .filter_map(|values| {
                (values.len() >= 3).then(|| Vec3::new(values[0], values[1], values[2]))
            })
            .collect(),
    )
}

fn parse_uv_array(text: &str, marker: &str) -> Option<Vec<[f32; 2]>> {
    let array = extract_usd_array(text, marker)?;
    Some(
        parse_usd_float_tuples(array)
            .into_iter()
            .filter_map(|values| (values.len() >= 2).then(|| [values[0], values[1]]))
            .collect(),
    )
}

fn parse_usd_int_array(text: &str, marker: &str) -> Option<Vec<usize>> {
    let array = extract_usd_array(text, marker)?;
    Some(
        array
            .split(|ch: char| ch == '[' || ch == ']' || ch == ',' || ch.is_whitespace())
            .filter_map(|part| {
                let part = part.trim();
                (!part.is_empty())
                    .then(|| part.parse::<i32>().ok())
                    .flatten()
            })
            .map(|value| value.max(0) as usize)
            .collect(),
    )
}

fn parse_usd_float_tuples(array: &str) -> Vec<Vec<f32>> {
    let mut tuples = Vec::new();
    let mut rest = array;
    while let Some(open) = rest.find('(') {
        let after_open = &rest[open + 1..];
        let Some(close) = after_open.find(')') else {
            break;
        };
        let tuple = after_open[..close]
            .split(',')
            .filter_map(|value| value.trim().parse::<f32>().ok())
            .collect::<Vec<_>>();
        if !tuple.is_empty() {
            tuples.push(tuple);
        }
        rest = &after_open[close + 1..];
    }
    tuples
}

fn extract_usd_array<'a>(text: &'a str, marker: &str) -> Option<&'a str> {
    let marker_start = text.find(marker)?;
    let search_start = marker_start + marker.len();
    let open = search_start + text[search_start..].find('[')?;
    let mut depth = 0usize;
    for (offset, ch) in text[open..].char_indices() {
        match ch {
            '[' => depth += 1,
            ']' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return text.get(open..open + offset + ch.len_utf8());
                }
            }
            _ => {}
        }
    }
    None
}

fn usdz_uv(
    uvs: &[[f32; 2]],
    point_count: usize,
    polygon_vertex_count: usize,
    point_index: usize,
    polygon_vertex: usize,
) -> [f32; 2] {
    if uvs.len() == point_count {
        uvs.get(point_index).copied().unwrap_or([0.0, 0.0])
    } else if uvs.len() == polygon_vertex_count {
        uvs.get(polygon_vertex).copied().unwrap_or([0.0, 0.0])
    } else {
        uvs.get(point_index)
            .or_else(|| uvs.get(polygon_vertex))
            .copied()
            .unwrap_or([0.0, 0.0])
    }
}

fn load_3ds(path: &Path) -> Result<Model, String> {
    let bytes = std::fs::read(path).map_err(|err| err.to_string())?;
    let file_size = bytes.len() as u64;
    let mut materials = HashMap::<String, ObjMaterial>::new();
    let mut textures = Vec::<Texture>::new();
    let mut meshes = Vec::<ThreeDsMesh>::new();

    parse_3ds_chunks(
        &bytes,
        0,
        bytes.len(),
        path,
        &mut materials,
        &mut textures,
        &mut meshes,
    )?;

    let fallback_material = (materials.len() == 1)
        .then(|| materials.values().next().copied())
        .flatten()
        .unwrap_or_default();
    let mut triangles = Vec::<Triangle>::new();
    let mut vertices = 0usize;
    for mesh in &meshes {
        vertices += mesh.positions.len();
        append_3ds_mesh(mesh, &materials, fallback_material, &mut triangles);
    }

    let texture_count = textures.len();
    build_model(
        path,
        "3ds",
        file_size,
        triangles,
        textures,
        ModelCounts {
            scenes: 1,
            nodes: meshes.len(),
            meshes: meshes.len(),
            materials: materials.len(),
            textures: texture_count,
            animations: 0,
            primitives: meshes.len(),
            vertices,
        },
    )
}

struct ThreeDsMesh {
    positions: Vec<Vec3>,
    uvs: Vec<[f32; 2]>,
    faces: Vec<[usize; 3]>,
    face_materials: Vec<Option<String>>,
}

impl ThreeDsMesh {
    fn new() -> Self {
        Self {
            positions: Vec::new(),
            uvs: Vec::new(),
            faces: Vec::new(),
            face_materials: Vec::new(),
        }
    }
}

fn parse_3ds_chunks(
    bytes: &[u8],
    start: usize,
    end: usize,
    path: &Path,
    materials: &mut HashMap<String, ObjMaterial>,
    textures: &mut Vec<Texture>,
    meshes: &mut Vec<ThreeDsMesh>,
) -> Result<(), String> {
    let mut offset = start;
    while offset + 6 <= end {
        let id = read_u16_at(bytes, offset)?;
        let len = read_u32_at(bytes, offset + 2)? as usize;
        if len < 6 {
            return Err(format!("invalid 3DS chunk length at offset {offset}"));
        }
        let chunk_start = offset + 6;
        let chunk_end = offset
            .checked_add(len)
            .ok_or("3DS chunk length overflow")?
            .min(end);
        match id {
            0xAFFF => {
                if let Some((name, material)) =
                    parse_3ds_material(bytes, chunk_start, chunk_end, path, textures)?
                {
                    materials.insert(name, material);
                }
            }
            0x4000 => {
                if let Some(mesh) = parse_3ds_object(bytes, chunk_start, chunk_end)? {
                    meshes.push(mesh);
                }
            }
            _ => parse_3ds_chunks(
                bytes,
                chunk_start,
                chunk_end,
                path,
                materials,
                textures,
                meshes,
            )?,
        }
        offset += len;
    }
    Ok(())
}

fn parse_3ds_material(
    bytes: &[u8],
    start: usize,
    end: usize,
    path: &Path,
    textures: &mut Vec<Texture>,
) -> Result<Option<(String, ObjMaterial)>, String> {
    let mut name = None::<String>;
    let mut material = ObjMaterial::default();
    let mut offset = start;
    while offset + 6 <= end {
        let id = read_u16_at(bytes, offset)?;
        let len = read_u32_at(bytes, offset + 2)? as usize;
        if len < 6 {
            return Err(format!(
                "invalid 3DS material chunk length at offset {offset}"
            ));
        }
        let chunk_start = offset + 6;
        let chunk_end = (offset + len).min(end);
        match id {
            0xA000 => {
                let mut cursor = chunk_start;
                name = Some(read_3ds_string(bytes, &mut cursor, chunk_end)?);
            }
            0xA020 => {
                if let Some(color) = parse_3ds_color(bytes, chunk_start, chunk_end)? {
                    material.color = color;
                }
            }
            0xA200 => {
                if let Some(texture_name) = parse_3ds_texture_name(bytes, chunk_start, chunk_end)? {
                    let texture_path = resolve_sibling_path(path, &texture_name);
                    if let Some(texture) = decode_texture_file(&texture_path)? {
                        let index = textures.len();
                        textures.push(texture);
                        material.texture = Some(index);
                    }
                }
            }
            _ => {}
        }
        offset += len;
    }
    Ok(name.map(|name| (name, material)))
}

fn parse_3ds_color(bytes: &[u8], start: usize, end: usize) -> Result<Option<Rgb>, String> {
    let mut offset = start;
    while offset + 6 <= end {
        let id = read_u16_at(bytes, offset)?;
        let len = read_u32_at(bytes, offset + 2)? as usize;
        let chunk_start = offset + 6;
        let chunk_end = (offset + len).min(end);
        match id {
            0x0010 if chunk_start + 12 <= chunk_end => {
                return Ok(Some(Rgb {
                    r: linear_to_u8(read_f32_at(bytes, chunk_start)?),
                    g: linear_to_u8(read_f32_at(bytes, chunk_start + 4)?),
                    b: linear_to_u8(read_f32_at(bytes, chunk_start + 8)?),
                }));
            }
            0x0011 | 0x0012 if chunk_start + 3 <= chunk_end => {
                return Ok(Some(Rgb {
                    r: bytes[chunk_start],
                    g: bytes[chunk_start + 1],
                    b: bytes[chunk_start + 2],
                }));
            }
            _ => {}
        }
        offset += len.max(6);
    }
    Ok(None)
}

fn parse_3ds_texture_name(
    bytes: &[u8],
    start: usize,
    end: usize,
) -> Result<Option<String>, String> {
    let mut offset = start;
    while offset + 6 <= end {
        let id = read_u16_at(bytes, offset)?;
        let len = read_u32_at(bytes, offset + 2)? as usize;
        let chunk_start = offset + 6;
        let chunk_end = (offset + len).min(end);
        if id == 0xA300 {
            let mut cursor = chunk_start;
            return Ok(Some(read_3ds_string(bytes, &mut cursor, chunk_end)?));
        }
        offset += len.max(6);
    }
    Ok(None)
}

fn parse_3ds_object(bytes: &[u8], start: usize, end: usize) -> Result<Option<ThreeDsMesh>, String> {
    let mut offset = start;
    let _name = read_3ds_string(bytes, &mut offset, end)?;
    let mut mesh = None::<ThreeDsMesh>;
    while offset + 6 <= end {
        let id = read_u16_at(bytes, offset)?;
        let len = read_u32_at(bytes, offset + 2)? as usize;
        if len < 6 {
            return Err(format!(
                "invalid 3DS object chunk length at offset {offset}"
            ));
        }
        let chunk_start = offset + 6;
        let chunk_end = (offset + len).min(end);
        if id == 0x4100 {
            mesh = Some(parse_3ds_mesh(bytes, chunk_start, chunk_end)?);
        }
        offset += len;
    }
    Ok(mesh.filter(|mesh| !mesh.positions.is_empty() && !mesh.faces.is_empty()))
}

fn parse_3ds_mesh(bytes: &[u8], start: usize, end: usize) -> Result<ThreeDsMesh, String> {
    let mut mesh = ThreeDsMesh::new();
    let mut offset = start;
    while offset + 6 <= end {
        let id = read_u16_at(bytes, offset)?;
        let len = read_u32_at(bytes, offset + 2)? as usize;
        if len < 6 {
            return Err(format!("invalid 3DS mesh chunk length at offset {offset}"));
        }
        let chunk_start = offset + 6;
        let chunk_end = (offset + len).min(end);
        match id {
            0x4110 => mesh.positions = parse_3ds_vertices(bytes, chunk_start, chunk_end)?,
            0x4120 => parse_3ds_faces(bytes, chunk_start, chunk_end, &mut mesh)?,
            0x4140 => mesh.uvs = parse_3ds_uvs(bytes, chunk_start, chunk_end)?,
            _ => {}
        }
        offset += len;
    }
    Ok(mesh)
}

fn parse_3ds_vertices(bytes: &[u8], start: usize, end: usize) -> Result<Vec<Vec3>, String> {
    if start + 2 > end {
        return Ok(Vec::new());
    }
    let count = read_u16_at(bytes, start)? as usize;
    let mut vertices = Vec::with_capacity(count);
    let mut offset = start + 2;
    for _ in 0..count {
        if offset + 12 > end {
            break;
        }
        vertices.push(Vec3::new(
            read_f32_at(bytes, offset)?,
            read_f32_at(bytes, offset + 4)?,
            read_f32_at(bytes, offset + 8)?,
        ));
        offset += 12;
    }
    Ok(vertices)
}

fn parse_3ds_faces(
    bytes: &[u8],
    start: usize,
    end: usize,
    mesh: &mut ThreeDsMesh,
) -> Result<(), String> {
    if start + 2 > end {
        return Ok(());
    }
    let count = read_u16_at(bytes, start)? as usize;
    mesh.faces.clear();
    mesh.face_materials.clear();
    mesh.faces.reserve(count);
    mesh.face_materials.resize(count, None);
    let mut offset = start + 2;
    for _ in 0..count {
        if offset + 8 > end {
            break;
        }
        mesh.faces.push([
            read_u16_at(bytes, offset)? as usize,
            read_u16_at(bytes, offset + 2)? as usize,
            read_u16_at(bytes, offset + 4)? as usize,
        ]);
        offset += 8;
    }
    while offset + 6 <= end {
        let id = read_u16_at(bytes, offset)?;
        let len = read_u32_at(bytes, offset + 2)? as usize;
        if len < 6 {
            return Err(format!(
                "invalid 3DS face subchunk length at offset {offset}"
            ));
        }
        let chunk_start = offset + 6;
        let chunk_end = (offset + len).min(end);
        if id == 0x4130 {
            parse_3ds_face_material(bytes, chunk_start, chunk_end, mesh)?;
        }
        offset += len;
    }
    Ok(())
}

fn parse_3ds_face_material(
    bytes: &[u8],
    start: usize,
    end: usize,
    mesh: &mut ThreeDsMesh,
) -> Result<(), String> {
    let mut offset = start;
    let name = read_3ds_string(bytes, &mut offset, end)?;
    if offset + 2 > end {
        return Ok(());
    }
    let count = read_u16_at(bytes, offset)? as usize;
    offset += 2;
    for _ in 0..count {
        if offset + 2 > end {
            break;
        }
        let face_index = read_u16_at(bytes, offset)? as usize;
        if let Some(slot) = mesh.face_materials.get_mut(face_index) {
            *slot = Some(name.clone());
        }
        offset += 2;
    }
    Ok(())
}

fn parse_3ds_uvs(bytes: &[u8], start: usize, end: usize) -> Result<Vec<[f32; 2]>, String> {
    if start + 2 > end {
        return Ok(Vec::new());
    }
    let count = read_u16_at(bytes, start)? as usize;
    let mut uvs = Vec::with_capacity(count);
    let mut offset = start + 2;
    for _ in 0..count {
        if offset + 8 > end {
            break;
        }
        uvs.push([read_f32_at(bytes, offset)?, read_f32_at(bytes, offset + 4)?]);
        offset += 8;
    }
    Ok(uvs)
}

fn append_3ds_mesh(
    mesh: &ThreeDsMesh,
    materials: &HashMap<String, ObjMaterial>,
    fallback_material: ObjMaterial,
    triangles: &mut Vec<Triangle>,
) {
    for (face_index, face) in mesh.faces.iter().enumerate() {
        if face.iter().any(|index| *index >= mesh.positions.len()) {
            continue;
        }
        let material = mesh
            .face_materials
            .get(face_index)
            .and_then(|name| name.as_ref())
            .and_then(|name| materials.get(name))
            .copied()
            .unwrap_or(fallback_material);
        let mut verts = [Vertex {
            position: Vec3::zero(),
            normal: Vec3::zero(),
            uv: [0.0, 0.0],
        }; 3];
        for (slot, index) in face.iter().enumerate() {
            verts[slot].position = mesh.positions[*index];
            verts[slot].uv = mesh.uvs.get(*index).copied().unwrap_or([0.0, 0.0]);
        }
        fill_missing_normals(&mut verts);
        triangles.push(Triangle {
            vertices: verts,
            color: material.color,
            texture: material.texture,
        });
    }
}

fn read_3ds_string(bytes: &[u8], offset: &mut usize, end: usize) -> Result<String, String> {
    let start = *offset;
    while *offset < end && bytes[*offset] != 0 {
        *offset += 1;
    }
    if *offset >= end {
        return Err("unterminated 3DS string".into());
    }
    let value = String::from_utf8_lossy(&bytes[start..*offset]).to_string();
    *offset += 1;
    Ok(value)
}

fn load_fbx(path: &Path) -> Result<Model, String> {
    let bytes = std::fs::read(path).map_err(|err| err.to_string())?;
    if !bytes.starts_with(b"Kaydara FBX Binary  \0\x1a\0") {
        return Err("only binary FBX files are currently supported".into());
    }
    let file_size = bytes.len() as u64;
    let version = read_u32_at(&bytes, 23)? as usize;
    let mut cursor = Cursor::new(bytes.as_slice());
    cursor.set_position(27);
    let mut geometries = Vec::<FbxGeometry>::new();
    while (cursor.position() as usize) < bytes.len() {
        let Some(node) = read_fbx_node(&mut cursor, version)? else {
            break;
        };
        collect_fbx_geometries(&node, &mut geometries);
    }

    let mut triangles = Vec::<Triangle>::new();
    let mut vertices = 0usize;
    let textures = load_sidecar_png_texture(path)
        .map(|texture| vec![texture])
        .unwrap_or_default();
    let texture_index = (!textures.is_empty()).then_some(0usize);
    for geometry in &geometries {
        vertices += geometry.positions.len();
        append_fbx_geometry(geometry, texture_index, &mut triangles);
    }

    let texture_count = textures.len();
    build_model(
        path,
        "fbx",
        file_size,
        triangles,
        textures,
        ModelCounts {
            scenes: 1,
            nodes: 0,
            meshes: geometries.len(),
            materials: 0,
            textures: texture_count,
            animations: 0,
            primitives: geometries.len(),
            vertices,
        },
    )
}

struct FbxGeometry {
    positions: Vec<Vec3>,
    indices: Vec<i32>,
    uv_set: Option<FbxUvSet>,
}

struct FbxUvSet {
    uvs: Vec<[f32; 2]>,
    indices: Vec<i32>,
    mapping: FbxUvMapping,
    reference: FbxUvReference,
}

#[derive(Clone, Copy)]
enum FbxUvMapping {
    ByPolygonVertex,
    ByVertex,
}

#[derive(Clone, Copy)]
enum FbxUvReference {
    Direct,
    IndexToDirect,
}

struct FbxNode {
    name: String,
    properties: Vec<FbxProperty>,
    children: Vec<FbxNode>,
}

enum FbxProperty {
    Ignored,
    String(String),
    I32Array(Vec<i32>),
    F64Array(Vec<f64>),
}

fn read_fbx_node(cursor: &mut Cursor<&[u8]>, version: usize) -> Result<Option<FbxNode>, String> {
    let start = cursor.position();
    let end_offset = if version >= 7500 {
        read_u64(cursor)? as u64
    } else {
        read_u32(cursor)? as u64
    };
    let property_count = if version >= 7500 {
        read_u64(cursor)? as usize
    } else {
        read_u32(cursor)? as usize
    };
    let _property_len = if version >= 7500 {
        read_u64(cursor)? as usize
    } else {
        read_u32(cursor)? as usize
    };
    let name_len = read_u8(cursor)? as usize;
    if end_offset == 0 && property_count == 0 && name_len == 0 {
        return Ok(None);
    }

    let mut name = vec![0u8; name_len];
    cursor
        .read_exact(&mut name)
        .map_err(|err| err.to_string())?;
    let name = String::from_utf8_lossy(&name).to_string();

    let mut properties = Vec::with_capacity(property_count);
    for _ in 0..property_count {
        properties.push(read_fbx_property(cursor)?);
    }

    let mut children = Vec::new();
    while cursor.position() < end_offset {
        let before = cursor.position();
        let Some(child) = read_fbx_node(cursor, version)? else {
            break;
        };
        children.push(child);
        if cursor.position() == before {
            break;
        }
    }
    cursor.set_position(end_offset.max(start));
    Ok(Some(FbxNode {
        name,
        properties,
        children,
    }))
}

fn read_fbx_property(cursor: &mut Cursor<&[u8]>) -> Result<FbxProperty, String> {
    let kind = read_u8(cursor)? as char;
    match kind {
        'Y' => {
            let _ = read_i16(cursor)?;
            Ok(FbxProperty::Ignored)
        }
        'C' => {
            let _ = read_u8(cursor)?;
            Ok(FbxProperty::Ignored)
        }
        'I' => {
            let _ = read_i32(cursor)?;
            Ok(FbxProperty::Ignored)
        }
        'L' => {
            let _ = read_i64(cursor)?;
            Ok(FbxProperty::Ignored)
        }
        'F' => {
            let _ = read_f32(cursor)?;
            Ok(FbxProperty::Ignored)
        }
        'D' => {
            let _ = read_f64(cursor)?;
            Ok(FbxProperty::Ignored)
        }
        'S' => {
            let len = read_u32(cursor)? as usize;
            let mut bytes = vec![0u8; len];
            cursor
                .read_exact(&mut bytes)
                .map_err(|err| err.to_string())?;
            Ok(FbxProperty::String(
                String::from_utf8_lossy(&bytes).to_string(),
            ))
        }
        'R' => {
            let len = read_u32(cursor)? as usize;
            let mut bytes = vec![0u8; len];
            cursor
                .read_exact(&mut bytes)
                .map_err(|err| err.to_string())?;
            Ok(FbxProperty::Ignored)
        }
        'i' => Ok(FbxProperty::I32Array(read_fbx_i32_array(cursor)?)),
        'd' => Ok(FbxProperty::F64Array(read_fbx_f64_array(cursor)?)),
        'f' => Ok(FbxProperty::F64Array(
            read_fbx_f32_array(cursor)?
                .into_iter()
                .map(f64::from)
                .collect(),
        )),
        other => Err(format!("unsupported FBX property type: {other}")),
    }
}

fn read_fbx_i32_array(cursor: &mut Cursor<&[u8]>) -> Result<Vec<i32>, String> {
    let bytes = read_fbx_array_bytes(cursor, 4)?;
    Ok(bytes
        .chunks_exact(4)
        .map(|chunk| i32::from_le_bytes(chunk.try_into().unwrap()))
        .collect())
}

fn read_fbx_f32_array(cursor: &mut Cursor<&[u8]>) -> Result<Vec<f32>, String> {
    let bytes = read_fbx_array_bytes(cursor, 4)?;
    Ok(bytes
        .chunks_exact(4)
        .map(|chunk| f32::from_le_bytes(chunk.try_into().unwrap()))
        .collect())
}

fn read_fbx_f64_array(cursor: &mut Cursor<&[u8]>) -> Result<Vec<f64>, String> {
    let bytes = read_fbx_array_bytes(cursor, 8)?;
    Ok(bytes
        .chunks_exact(8)
        .map(|chunk| f64::from_le_bytes(chunk.try_into().unwrap()))
        .collect())
}

fn read_fbx_array_bytes(cursor: &mut Cursor<&[u8]>, item_size: usize) -> Result<Vec<u8>, String> {
    let len = read_u32(cursor)? as usize;
    let encoding = read_u32(cursor)?;
    let byte_len = read_u32(cursor)? as usize;
    let mut bytes = vec![0u8; byte_len];
    cursor
        .read_exact(&mut bytes)
        .map_err(|err| err.to_string())?;
    if encoding == 0 {
        return Ok(bytes);
    }
    if encoding != 1 {
        return Err(format!("unsupported FBX array encoding: {encoding}"));
    }
    let mut decoder = ZlibDecoder::new(bytes.as_slice());
    let mut decoded = Vec::with_capacity(len * item_size);
    decoder
        .read_to_end(&mut decoded)
        .map_err(|err| err.to_string())?;
    Ok(decoded)
}

fn collect_fbx_geometries(node: &FbxNode, geometries: &mut Vec<FbxGeometry>) {
    if node.name == "Geometry" {
        let mut positions = None::<Vec<Vec3>>;
        let mut indices = None::<Vec<i32>>;
        let mut uv_set = None::<FbxUvSet>;
        for child in &node.children {
            match child.name.as_str() {
                "Vertices" => {
                    if let Some(FbxProperty::F64Array(values)) = child.properties.first() {
                        positions = Some(
                            values
                                .chunks_exact(3)
                                .map(|chunk| {
                                    Vec3::new(chunk[0] as f32, chunk[1] as f32, chunk[2] as f32)
                                })
                                .collect(),
                        );
                    }
                }
                "PolygonVertexIndex" => {
                    if let Some(FbxProperty::I32Array(values)) = child.properties.first() {
                        indices = Some(values.clone());
                    }
                }
                "LayerElementUV" => {
                    if uv_set.is_none() {
                        uv_set = collect_fbx_uvs(child);
                    }
                }
                _ => {}
            }
        }
        if let (Some(positions), Some(indices)) = (positions, indices) {
            geometries.push(FbxGeometry {
                positions,
                indices,
                uv_set,
            });
        }
    }
    for child in &node.children {
        collect_fbx_geometries(child, geometries);
    }
}

fn collect_fbx_uvs(node: &FbxNode) -> Option<FbxUvSet> {
    let mut mapping = None::<&str>;
    let mut reference = None::<&str>;
    let mut uvs = Vec::<[f32; 2]>::new();
    let mut indices = Vec::<i32>::new();
    for child in &node.children {
        match child.name.as_str() {
            "MappingInformationType" => {
                if let Some(FbxProperty::String(value)) = child.properties.first() {
                    mapping = Some(value.as_str());
                }
            }
            "ReferenceInformationType" => {
                if let Some(FbxProperty::String(value)) = child.properties.first() {
                    reference = Some(value.as_str());
                }
            }
            "UV" => {
                if let Some(FbxProperty::F64Array(values)) = child.properties.first() {
                    uvs = values
                        .chunks_exact(2)
                        .map(|chunk| [chunk[0] as f32, chunk[1] as f32])
                        .collect();
                }
            }
            "UVIndex" => {
                if let Some(FbxProperty::I32Array(values)) = child.properties.first() {
                    indices = values.clone();
                }
            }
            _ => {}
        }
    }

    if uvs.is_empty() {
        return None;
    }

    Some(FbxUvSet {
        uvs,
        indices,
        mapping: match mapping {
            Some("ByVertice" | "ByVertex") => FbxUvMapping::ByVertex,
            Some("ByPolygonVertex") | None => FbxUvMapping::ByPolygonVertex,
            _ => return None,
        },
        reference: match reference {
            Some("Direct") => FbxUvReference::Direct,
            Some("IndexToDirect" | "Index") | None => FbxUvReference::IndexToDirect,
            _ => return None,
        },
    })
}

#[derive(Clone, Copy)]
struct FbxCorner {
    position: usize,
    uv: [f32; 2],
}

fn append_fbx_geometry(
    geometry: &FbxGeometry,
    texture: Option<usize>,
    triangles: &mut Vec<Triangle>,
) {
    let mut polygon = Vec::<FbxCorner>::new();
    let mut polygon_vertex = 0usize;
    for raw in &geometry.indices {
        let last = *raw < 0;
        let index = if last {
            (-raw - 1) as usize
        } else {
            *raw as usize
        };
        if index < geometry.positions.len() {
            polygon.push(FbxCorner {
                position: index,
                uv: fbx_corner_uv(geometry, index, polygon_vertex),
            });
        }
        polygon_vertex += 1;
        if last {
            if polygon.len() >= 3 {
                for i in 1..polygon.len() - 1 {
                    let indices = [polygon[0], polygon[i], polygon[i + 1]];
                    let mut verts = [Vertex {
                        position: Vec3::zero(),
                        normal: Vec3::zero(),
                        uv: [0.0, 0.0],
                    }; 3];
                    for (slot, corner) in indices.iter().enumerate() {
                        verts[slot].position = geometry.positions[corner.position];
                        verts[slot].uv = corner.uv;
                    }
                    fill_missing_normals(&mut verts);
                    triangles.push(Triangle {
                        vertices: verts,
                        color: if texture.is_some() {
                            Rgb {
                                r: 255,
                                g: 255,
                                b: 255,
                            }
                        } else {
                            default_color()
                        },
                        texture,
                    });
                }
            }
            polygon.clear();
        }
    }
}

fn fbx_corner_uv(geometry: &FbxGeometry, position: usize, polygon_vertex: usize) -> [f32; 2] {
    let Some(uv_set) = &geometry.uv_set else {
        return [0.0, 0.0];
    };
    let source_index = match uv_set.mapping {
        FbxUvMapping::ByPolygonVertex => polygon_vertex,
        FbxUvMapping::ByVertex => position,
    };
    let uv_index = match uv_set.reference {
        FbxUvReference::Direct => source_index,
        FbxUvReference::IndexToDirect => uv_set
            .indices
            .get(source_index)
            .copied()
            .map(|value| value.max(0) as usize)
            .unwrap_or(source_index),
    };
    uv_set.uvs.get(uv_index).copied().unwrap_or([0.0, 0.0])
}

struct ModelCounts {
    scenes: usize,
    nodes: usize,
    meshes: usize,
    materials: usize,
    textures: usize,
    animations: usize,
    primitives: usize,
    vertices: usize,
}

fn build_model(
    path: &Path,
    format: &str,
    file_size: u64,
    triangles: Vec<Triangle>,
    textures: Vec<Texture>,
    counts: ModelCounts,
) -> Result<Model, String> {
    if triangles.is_empty() {
        return Err("model has no triangle mesh primitives".into());
    }

    let mut min = Vec3::splat(f32::INFINITY);
    let mut max = Vec3::splat(f32::NEG_INFINITY);
    for triangle in &triangles {
        for vertex in &triangle.vertices {
            min = min.min(vertex.position);
            max = max.max(vertex.position);
        }
    }
    let center = (min + max) * 0.5;
    let radius = (max - center).length().max(0.001);
    let meta = ModelMeta {
        file_name: path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("model")
            .to_string(),
        format: format.to_string(),
        file_size,
        scenes: counts.scenes,
        nodes: counts.nodes,
        meshes: counts.meshes,
        materials: counts.materials,
        textures: counts.textures,
        animations: counts.animations,
        primitives: counts.primitives,
        vertices: counts.vertices,
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

fn fill_missing_normals(vertices: &mut [Vertex; 3]) {
    if vertices
        .iter()
        .all(|vertex| vertex.normal.length_squared() > 0.0)
    {
        return;
    }
    let face = (vertices[1].position - vertices[0].position)
        .cross(vertices[2].position - vertices[0].position)
        .normalize_or(Vec3::new(0.0, 1.0, 0.0));
    for vertex in vertices {
        if vertex.normal.length_squared() == 0.0 {
            vertex.normal = face;
        }
    }
}

fn default_color() -> Rgb {
    Rgb {
        r: 190,
        g: 190,
        b: 190,
    }
}

fn read_u8(cursor: &mut Cursor<&[u8]>) -> Result<u8, String> {
    let mut bytes = [0u8; 1];
    cursor
        .read_exact(&mut bytes)
        .map_err(|err| err.to_string())?;
    Ok(bytes[0])
}

fn read_i16(cursor: &mut Cursor<&[u8]>) -> Result<i16, String> {
    let mut bytes = [0u8; 2];
    cursor
        .read_exact(&mut bytes)
        .map_err(|err| err.to_string())?;
    Ok(i16::from_le_bytes(bytes))
}

fn read_u32(cursor: &mut Cursor<&[u8]>) -> Result<u32, String> {
    let mut bytes = [0u8; 4];
    cursor
        .read_exact(&mut bytes)
        .map_err(|err| err.to_string())?;
    Ok(u32::from_le_bytes(bytes))
}

fn read_i32(cursor: &mut Cursor<&[u8]>) -> Result<i32, String> {
    let mut bytes = [0u8; 4];
    cursor
        .read_exact(&mut bytes)
        .map_err(|err| err.to_string())?;
    Ok(i32::from_le_bytes(bytes))
}

fn read_u64(cursor: &mut Cursor<&[u8]>) -> Result<u64, String> {
    let mut bytes = [0u8; 8];
    cursor
        .read_exact(&mut bytes)
        .map_err(|err| err.to_string())?;
    Ok(u64::from_le_bytes(bytes))
}

fn read_i64(cursor: &mut Cursor<&[u8]>) -> Result<i64, String> {
    let mut bytes = [0u8; 8];
    cursor
        .read_exact(&mut bytes)
        .map_err(|err| err.to_string())?;
    Ok(i64::from_le_bytes(bytes))
}

fn read_f32(cursor: &mut Cursor<&[u8]>) -> Result<f32, String> {
    let mut bytes = [0u8; 4];
    cursor
        .read_exact(&mut bytes)
        .map_err(|err| err.to_string())?;
    Ok(f32::from_le_bytes(bytes))
}

fn read_f64(cursor: &mut Cursor<&[u8]>) -> Result<f64, String> {
    let mut bytes = [0u8; 8];
    cursor
        .read_exact(&mut bytes)
        .map_err(|err| err.to_string())?;
    Ok(f64::from_le_bytes(bytes))
}

fn read_u32_at(bytes: &[u8], offset: usize) -> Result<u32, String> {
    let bytes = bytes
        .get(offset..offset + 4)
        .ok_or("unexpected end of FBX header")?;
    Ok(u32::from_le_bytes(bytes.try_into().unwrap()))
}

fn read_u16_at(bytes: &[u8], offset: usize) -> Result<u16, String> {
    let bytes = bytes
        .get(offset..offset + 2)
        .ok_or("unexpected end of binary model data")?;
    Ok(u16::from_le_bytes(bytes.try_into().unwrap()))
}

fn read_f32_at(bytes: &[u8], offset: usize) -> Result<f32, String> {
    let bytes = bytes
        .get(offset..offset + 4)
        .ok_or("unexpected end of binary model data")?;
    Ok(f32::from_le_bytes(bytes.try_into().unwrap()))
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
    Ok(texture_from_pixels(width, height, pixels))
}

fn load_sidecar_png_texture(path: &Path) -> Option<Texture> {
    let texture_path = path.with_extension("png");
    if !texture_path.is_file() {
        return None;
    }
    decode_png_texture(&texture_path).ok()
}

fn decode_texture_file(path: &Path) -> Result<Option<Texture>, String> {
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match extension.as_str() {
        "png" => decode_png_texture(path).map(Some),
        "jpg" | "jpeg" => decode_jpeg_texture(path).map(Some),
        _ => Ok(None),
    }
}

fn decode_png_texture(path: &Path) -> Result<Texture, String> {
    let file = std::fs::File::open(path).map_err(|err| err.to_string())?;
    let decoder = png::Decoder::new(file);
    let mut reader = decoder.read_info().map_err(|err| err.to_string())?;
    let mut buffer = vec![0; reader.output_buffer_size()];
    let info = reader
        .next_frame(&mut buffer)
        .map_err(|err| err.to_string())?;
    let bytes = &buffer[..info.buffer_size()];
    let pixels = match info.color_type {
        png::ColorType::Rgb => bytes
            .chunks_exact(3)
            .map(|chunk| Rgb {
                r: chunk[0],
                g: chunk[1],
                b: chunk[2],
            })
            .collect(),
        png::ColorType::Rgba => bytes
            .chunks_exact(4)
            .map(|chunk| Rgb {
                r: chunk[0],
                g: chunk[1],
                b: chunk[2],
            })
            .collect(),
        png::ColorType::Grayscale => bytes
            .iter()
            .map(|value| Rgb {
                r: *value,
                g: *value,
                b: *value,
            })
            .collect(),
        png::ColorType::GrayscaleAlpha => bytes
            .chunks_exact(2)
            .map(|chunk| Rgb {
                r: chunk[0],
                g: chunk[0],
                b: chunk[0],
            })
            .collect(),
        color => {
            return Err(format!(
                "unsupported sidecar PNG color type for {}: {color:?}",
                path.display()
            ));
        }
    };
    Ok(texture_from_pixels(
        info.width as usize,
        info.height as usize,
        pixels,
    ))
}

fn decode_jpeg_texture(path: &Path) -> Result<Texture, String> {
    let file = std::fs::File::open(path).map_err(|err| err.to_string())?;
    let mut decoder = jpeg_decoder::Decoder::new(file);
    let pixels = decoder.decode().map_err(|err| err.to_string())?;
    let info = decoder
        .info()
        .ok_or_else(|| format!("missing JPEG metadata for {}", path.display()))?;
    let pixels = match info.pixel_format {
        jpeg_decoder::PixelFormat::L8 => pixels
            .iter()
            .map(|value| Rgb {
                r: *value,
                g: *value,
                b: *value,
            })
            .collect(),
        jpeg_decoder::PixelFormat::RGB24 => pixels
            .chunks_exact(3)
            .map(|chunk| Rgb {
                r: chunk[0],
                g: chunk[1],
                b: chunk[2],
            })
            .collect(),
        jpeg_decoder::PixelFormat::CMYK32 => pixels
            .chunks_exact(4)
            .map(|chunk| {
                let c = chunk[0] as u16;
                let m = chunk[1] as u16;
                let y = chunk[2] as u16;
                let k = chunk[3] as u16;
                Rgb {
                    r: (255 - ((c * (255 - k) + 127) / 255 + k).min(255)) as u8,
                    g: (255 - ((m * (255 - k) + 127) / 255 + k).min(255)) as u8,
                    b: (255 - ((y * (255 - k) + 127) / 255 + k).min(255)) as u8,
                }
            })
            .collect(),
        format => {
            return Err(format!(
                "unsupported JPEG color format for {}: {format:?}",
                path.display()
            ));
        }
    };
    Ok(texture_from_pixels(
        info.width as usize,
        info.height as usize,
        pixels,
    ))
}

fn texture_from_pixels(width: usize, height: usize, pixels: Vec<Rgb>) -> Texture {
    Texture {
        width,
        height,
        pixels,
    }
}
