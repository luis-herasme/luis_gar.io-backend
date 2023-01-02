use std::ops;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Copy)]
pub struct Vector2D {
    pub x: f32,
    pub y: f32,
}

impl Vector2D {
    pub fn new(x: f32, y: f32) -> Vector2D {
        Vector2D { x, y }
    }

    pub fn magnitude(&self) -> f32 {
        (self.x.powf(2.0) + self.y.powf(2.0)).sqrt()
    }

    pub fn normalize(&self) -> Vector2D {
        let magnitude = self.magnitude();

        if magnitude == 0.0 {
            return Vector2D::new(0.0, 0.0);
        }

        Vector2D {
            x: (self.x / magnitude),
            y: (self.y / magnitude),
        }
    }
}

impl ops::Add for Vector2D {
    type Output = Vector2D;

    fn add(self, other: Vector2D) -> Vector2D {
        Vector2D {
            x: self.x + other.x,
            y: self.y + other.y,
        }
    }
}

impl ops::Sub for Vector2D {
    type Output = Vector2D;

    fn sub(self, other: Vector2D) -> Vector2D {
        Vector2D {
            x: self.x - other.x,
            y: self.y - other.y,
        }
    }
}

impl ops::Mul<f32> for Vector2D {
    type Output = Vector2D;

    fn mul(self, other: f32) -> Vector2D {
        Vector2D {
            x: self.x * other,
            y: self.y * other,
        }
    }
}
