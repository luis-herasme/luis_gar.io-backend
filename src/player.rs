use crate::vector::Vector2D;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Player {
    pub id: u32,
    pub position: Vector2D,
    pub radius: f32,
    pub name: String,
}

impl Player {
    pub fn new(id: u32, name: String) -> Player {
        Player {
            id,
            name,
            position: Vector2D::new(0.0, 0.0),
            radius: 10.0,
        }
    }

    pub fn mass(&self) -> f32 {
        2.0 * self.radius.powf(2.0) * std::f32::consts::PI
    }

    pub fn radius_after_eat(player: &Player, other: &Player) -> f32 {
        let combined_mass = other.mass() + player.mass();
        (combined_mass / (2.0 * std::f32::consts::PI)).sqrt()
    }

    pub fn move_towards(&mut self, position: Vector2D) {
        let velocity = 100.0 / self.mass().sqrt();
        let mut difference = position - self.position;

        if difference.magnitude() < velocity {
            return;
        }

        difference = difference.normalize() * velocity;
        self.position = self.position + difference;
    }
}
