use crate::math::{Mat4, Vec3};
use crate::model::{Model, Texture};
use crate::protocols::{Image, Rgb};

pub const DEFAULT_BACKGROUND: Rgb = Rgb {
    r: 40,
    g: 45,
    b: 53,
};

#[derive(Clone, Copy, Debug)]
pub struct View {
    pub yaw: f32,
    pub pitch: f32,
    pub distance: f32,
}

#[derive(Clone, Copy, Debug)]
struct ScreenVertex {
    x: f32,
    y: f32,
    z: f32,
    normal: Vec3,
    uv: [f32; 2],
}

pub fn render_model(
    model: &Model,
    view: View,
    width: usize,
    height: usize,
    background: Rgb,
) -> Image {
    let mut pixels = vec![Some(background); width * height];
    let mut zbuf = vec![f32::INFINITY; width * height];
    let aspect = width as f32 / height.max(1) as f32;
    let fov = 42.0_f32.to_radians();
    let focal = 1.0 / (fov * 0.5).tan();
    let light = Vec3::new(-0.35, 0.75, 0.55).normalize_or(Vec3::new(0.0, 1.0, 0.0));
    let fill_light = Vec3::new(0.45, 0.2, -0.75).normalize_or(Vec3::new(0.0, 0.0, -1.0));
    let view_rotation = Mat4::rotation_x(view.pitch) * Mat4::rotation_y(view.yaw);
    for tri in &model.triangles {
        let mut projected = [ScreenVertex {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            normal: Vec3::zero(),
            uv: [0.0, 0.0],
        }; 3];
        let mut visible = true;
        for (slot, vertex) in tri.vertices.iter().enumerate() {
            let local = (vertex.position - model.center) * (1.0 / model.radius);
            let p = view_rotation.transform_vector(local) + Vec3::new(0.0, 0.0, view.distance);
            if p.z <= 0.05 {
                visible = false;
                break;
            }
            let ndc_x = (p.x * focal / aspect) / p.z;
            let ndc_y = (p.y * focal) / p.z;
            projected[slot] = ScreenVertex {
                x: (ndc_x * 0.5 + 0.5) * (width as f32 - 1.0),
                y: (0.5 - ndc_y * 0.5) * (height as f32 - 1.0),
                z: p.z,
                normal: view_rotation
                    .transform_vector(vertex.normal)
                    .normalize_or(Vec3::new(0.0, 1.0, 0.0)),
                uv: vertex.uv,
            };
        }
        if visible {
            let texture = tri.texture.and_then(|index| model.textures.get(index));
            raster_triangle(
                &projected,
                tri.color,
                texture,
                light,
                fill_light,
                width,
                height,
                &mut pixels,
                &mut zbuf,
            );
        }
    }

    Image {
        width,
        height,
        pixels,
    }
}

fn raster_triangle(
    v: &[ScreenVertex; 3],
    base: Rgb,
    texture: Option<&Texture>,
    light: Vec3,
    fill_light: Vec3,
    width: usize,
    height: usize,
    pixels: &mut [Option<Rgb>],
    zbuf: &mut [f32],
) {
    let area = edge(v[0], v[1], v[2].x, v[2].y);
    if area.abs() < 0.001 {
        return;
    }

    let min_x = v
        .iter()
        .map(|p| p.x.floor() as i32)
        .min()
        .unwrap()
        .clamp(0, width as i32 - 1);
    let max_x = v
        .iter()
        .map(|p| p.x.ceil() as i32)
        .max()
        .unwrap()
        .clamp(0, width as i32 - 1);
    let min_y = v
        .iter()
        .map(|p| p.y.floor() as i32)
        .min()
        .unwrap()
        .clamp(0, height as i32 - 1);
    let max_y = v
        .iter()
        .map(|p| p.y.ceil() as i32)
        .max()
        .unwrap()
        .clamp(0, height as i32 - 1);
    let sign = area.signum();

    for y in min_y..=max_y {
        for x in min_x..=max_x {
            let px = x as f32 + 0.5;
            let py = y as f32 + 0.5;
            let w0 = edge(v[1], v[2], px, py) * sign;
            let w1 = edge(v[2], v[0], px, py) * sign;
            let w2 = edge(v[0], v[1], px, py) * sign;
            if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
                continue;
            }
            let inv_area = 1.0 / area.abs();
            let b0 = w0 * inv_area;
            let b1 = w1 * inv_area;
            let b2 = w2 * inv_area;
            let z = b0 * v[0].z + b1 * v[1].z + b2 * v[2].z;
            let offset = y as usize * width + x as usize;
            if z >= zbuf[offset] {
                continue;
            }
            zbuf[offset] = z;
            let normal = (v[0].normal * b0 + v[1].normal * b1 + v[2].normal * b2)
                .normalize_or(Vec3::new(0.0, 1.0, 0.0));
            let key = normal.dot(light).abs();
            let fill = normal.dot(fill_light).abs();
            let facing = normal.z.abs();
            let shade = (0.46 + key * 0.34 + fill * 0.12 + facing * 0.08).min(1.08);
            let uv = [
                v[0].uv[0] * b0 + v[1].uv[0] * b1 + v[2].uv[0] * b2,
                v[0].uv[1] * b0 + v[1].uv[1] * b1 + v[2].uv[1] * b2,
            ];
            let color = texture
                .map(|texture| multiply_rgb(sample_texture(texture, uv), base))
                .unwrap_or(base);
            pixels[offset] = Some(scale_rgb(soft_lift_dark_color(color), shade));
        }
    }
}

fn edge(a: ScreenVertex, b: ScreenVertex, x: f32, y: f32) -> f32 {
    (x - a.x) * (b.y - a.y) - (y - a.y) * (b.x - a.x)
}

fn scale_rgb(color: Rgb, shade: f32) -> Rgb {
    Rgb {
        r: ((color.r as f32 * shade).clamp(0.0, 255.0)) as u8,
        g: ((color.g as f32 * shade).clamp(0.0, 255.0)) as u8,
        b: ((color.b as f32 * shade).clamp(0.0, 255.0)) as u8,
    }
}

fn sample_texture(texture: &Texture, uv: [f32; 2]) -> Rgb {
    if texture.width == 0 || texture.height == 0 || texture.pixels.is_empty() {
        return Rgb {
            r: 255,
            g: 255,
            b: 255,
        };
    }
    let u = uv[0].rem_euclid(1.0);
    let v = uv[1].rem_euclid(1.0);
    let x = ((u * texture.width as f32).floor() as usize).min(texture.width - 1);
    let y = (((1.0 - v) * texture.height as f32).floor() as usize).min(texture.height - 1);
    texture.pixels[y * texture.width + x]
}

fn multiply_rgb(a: Rgb, b: Rgb) -> Rgb {
    Rgb {
        r: ((a.r as u16 * b.r as u16) / 255) as u8,
        g: ((a.g as u16 * b.g as u16) / 255) as u8,
        b: ((a.b as u16 * b.b as u16) / 255) as u8,
    }
}

fn soft_lift_dark_color(color: Rgb) -> Rgb {
    let luminance = 0.2126 * color.r as f32 + 0.7152 * color.g as f32 + 0.0722 * color.b as f32;
    if luminance >= 65.0 {
        return color;
    }
    let lift = (65.0 - luminance) * 0.22;
    Rgb {
        r: (color.r as f32 + lift).min(180.0) as u8,
        g: (color.g as f32 + lift).min(180.0) as u8,
        b: (color.b as f32 + lift).min(180.0) as u8,
    }
}
