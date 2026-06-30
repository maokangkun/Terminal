#[derive(Clone, Copy, Debug)]
pub struct Vec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct Vec4 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub w: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct Mat4 {
    m: [[f32; 4]; 4],
}

impl Vec3 {
    pub fn new(x: f32, y: f32, z: f32) -> Self {
        Self { x, y, z }
    }

    pub fn zero() -> Self {
        Self::splat(0.0)
    }

    pub fn splat(value: f32) -> Self {
        Self {
            x: value,
            y: value,
            z: value,
        }
    }

    pub fn from_array(value: [f32; 3]) -> Self {
        Self::new(value[0], value[1], value[2])
    }

    pub fn dot(self, other: Self) -> f32 {
        self.x * other.x + self.y * other.y + self.z * other.z
    }

    pub fn cross(self, other: Self) -> Self {
        Self::new(
            self.y * other.z - self.z * other.y,
            self.z * other.x - self.x * other.z,
            self.x * other.y - self.y * other.x,
        )
    }

    pub fn length_squared(self) -> f32 {
        self.dot(self)
    }

    pub fn length(self) -> f32 {
        self.length_squared().sqrt()
    }

    pub fn normalize_or(self, fallback: Self) -> Self {
        let len = self.length();
        if len > 0.000001 {
            self * (1.0 / len)
        } else {
            fallback
        }
    }

    pub fn min(self, other: Self) -> Self {
        Self::new(
            self.x.min(other.x),
            self.y.min(other.y),
            self.z.min(other.z),
        )
    }

    pub fn max(self, other: Self) -> Self {
        Self::new(
            self.x.max(other.x),
            self.y.max(other.y),
            self.z.max(other.z),
        )
    }
}

impl std::ops::Add for Vec3 {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Self::new(self.x + rhs.x, self.y + rhs.y, self.z + rhs.z)
    }
}

impl std::ops::Sub for Vec3 {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        Self::new(self.x - rhs.x, self.y - rhs.y, self.z - rhs.z)
    }
}

impl std::ops::Mul<f32> for Vec3 {
    type Output = Self;

    fn mul(self, rhs: f32) -> Self::Output {
        Self::new(self.x * rhs, self.y * rhs, self.z * rhs)
    }
}

impl Mat4 {
    pub fn identity() -> Self {
        Self {
            m: [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
        }
    }

    pub fn from_array(m: [[f32; 4]; 4]) -> Self {
        Self { m }
    }

    pub fn rotation_x(angle: f32) -> Self {
        let (s, c) = angle.sin_cos();
        Self {
            m: [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, c, s, 0.0],
                [0.0, -s, c, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
        }
    }

    pub fn rotation_y(angle: f32) -> Self {
        let (s, c) = angle.sin_cos();
        Self {
            m: [
                [c, 0.0, -s, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [s, 0.0, c, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
        }
    }

    pub fn transform_point(self, p: Vec3) -> Vec3 {
        let out = self
            * Vec4 {
                x: p.x,
                y: p.y,
                z: p.z,
                w: 1.0,
            };
        if out.w.abs() > 0.000001 {
            Vec3::new(out.x / out.w, out.y / out.w, out.z / out.w)
        } else {
            Vec3::new(out.x, out.y, out.z)
        }
    }

    pub fn transform_vector(self, p: Vec3) -> Vec3 {
        let out = self
            * Vec4 {
                x: p.x,
                y: p.y,
                z: p.z,
                w: 0.0,
            };
        Vec3::new(out.x, out.y, out.z)
    }
}

impl std::ops::Mul for Mat4 {
    type Output = Self;

    fn mul(self, rhs: Self) -> Self::Output {
        let mut m = [[0.0; 4]; 4];
        for (row, out) in m.iter_mut().enumerate() {
            for (col, cell) in out.iter_mut().enumerate() {
                *cell = self.m[row][0] * rhs.m[0][col]
                    + self.m[row][1] * rhs.m[1][col]
                    + self.m[row][2] * rhs.m[2][col]
                    + self.m[row][3] * rhs.m[3][col];
            }
        }
        Self { m }
    }
}

impl std::ops::Mul<Vec4> for Mat4 {
    type Output = Vec4;

    fn mul(self, rhs: Vec4) -> Self::Output {
        Vec4 {
            x: self.m[0][0] * rhs.x
                + self.m[1][0] * rhs.y
                + self.m[2][0] * rhs.z
                + self.m[3][0] * rhs.w,
            y: self.m[0][1] * rhs.x
                + self.m[1][1] * rhs.y
                + self.m[2][1] * rhs.z
                + self.m[3][1] * rhs.w,
            z: self.m[0][2] * rhs.x
                + self.m[1][2] * rhs.y
                + self.m[2][2] * rhs.z
                + self.m[3][2] * rhs.w,
            w: self.m[0][3] * rhs.x
                + self.m[1][3] * rhs.y
                + self.m[2][3] * rhs.z
                + self.m[3][3] * rhs.w,
        }
    }
}
