use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket};
use futures::stream::SplitSink;
use futures::SinkExt;
use tokio::time::{self, Duration};

use crate::player::Player;
use crate::vector::Vector2D;
use rand::Rng;
use tokio::sync::mpsc::{Receiver, Sender};
use tokio::sync::{broadcast, mpsc, Mutex};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum PlayerCommand {
    Move { position: Vector2D },
    Join { name: String },
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PlayerMessage {
    pub id: u32,
    pub command: PlayerCommand,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum InternalCommand {
    Update,
    AddPlayer { id: u32, name: String },
    RemovePlayer { id: u32 },
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum Command {
    PlayerCommand(PlayerMessage),
    InternalCommand(InternalCommand),
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum MessageToClient {
    JoinSuccess {
        id: u32,
    },
    PlayerEaten {
        id: u32,
    },
    State {
        players: Vec<Player>,
        food: Vec<Food>,
    },
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Food {
    pub position: Vector2D,
    pub radius: f32,
}

pub struct GameManager {
    pub food: Vec<Food>,
    pub players: Vec<Player>,
    // Send messages to all the players
    pub broadcast_channel: tokio::sync::broadcast::Sender<MessageToClient>,
    // Receive and transmit commands, either from the websocket or from the update loop
    // the commands can be either internal or player commands
    pub command_rx: Receiver<Command>,
    pub command_tx: Sender<Command>,
    // Players sockets, used to send messages to specific players
    pub players_sockets: Arc<Mutex<HashMap<u32, Arc<Mutex<SplitSink<WebSocket, Message>>>>>>,
}

impl GameManager {
    pub fn new(broadcast_channel: broadcast::Sender<MessageToClient>) -> GameManager {
        let (command_tx, command_rx) = mpsc::channel::<Command>(100);
        let food = GameManager::generate_food(50);

        GameManager {
            food,
            players: Vec::new(),
            broadcast_channel,
            command_rx,
            command_tx,
            players_sockets: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn generate_food(amount: u32) -> Vec<Food> {
        let mut rng = rand::thread_rng();

        // generates a vector of food
        let mut food = Vec::new();
        for _ in 0..amount {
            let radius: f32 = rng.gen_range(2.0..6.0);
            let x: f32 = rng.gen_range(radius..800.0 - radius);
            let y: f32 = rng.gen_range(radius..600.0 - radius);

            food.push(Food {
                position: Vector2D::new(x, y),
                radius,
            });
        }
        food
    }

    fn send_string_to_player(&self, id: u32, message: String) {
        let players_sockets = self.players_sockets.clone();

        tokio::spawn(async move {
            let players_sockets = players_sockets.lock().await;
            let player_socket = players_sockets.get(&id);

            if let Some(player_socket) = player_socket {
                let mut player_socket = player_socket.lock().await;

                if let Err(error) = player_socket.send(Message::Text(message)).await {
                    println!("Error sending message to player: {}", error);
                }
            }
        });
    }

    pub fn send_message_to_player(&self, id: u32, message: MessageToClient) {
        let msg_string = serde_json::to_string::<MessageToClient>(&message);

        match msg_string {
            Ok(msg_string) => {
                self.send_string_to_player(id, String::from(msg_string));
            }
            Err(error) => {
                println!("Error serializing message: {}", error);
            }
        }
    }

    pub fn get_players(&self) -> Vec<Player> {
        self.players.clone()
    }

    pub fn start(self) {
        self.start_update_loop(10);
        GameManager::listen_to_commands(self);
    }

    pub fn listen_to_commands(mut game_manager: GameManager) {
        tokio::spawn(async move {
            loop {
                match game_manager.command_rx.recv().await {
                    Some(command) => match command {
                        Command::InternalCommand(internal_command) => {
                            game_manager.execute_internal_command(internal_command);
                        }
                        Command::PlayerCommand(player_command) => {
                            game_manager.execute_player_command(player_command);
                        }
                    },
                    None => {
                        println!("Error receiving command");
                        break;
                    }
                }
            }
        });
    }

    pub fn start_update_loop(&self, milliseconds: u64) {
        let command_tx_internal = self.command_tx.clone();
        tokio::spawn(async move {
            loop {
                time::sleep(Duration::from_millis(milliseconds)).await;
                if let Err(error) = command_tx_internal
                    .send(Command::InternalCommand(InternalCommand::Update))
                    .await
                {
                    println!("Error sending update command: {}", error);
                    break;
                }
            }
        });
    }

    pub fn send_state(&self) {
        let players = self.get_players();
        if let Err(error) = self.broadcast_channel.send(MessageToClient::State {
            players: players.clone(),
            food: self.food.clone(),
        }) {
            println!("Error sending state: {}", error);
        }
    }

    pub fn execute_internal_command(&mut self, internal_command: InternalCommand) {
        match internal_command {
            InternalCommand::Update => {
                self.update();
                self.send_state();
            }
            InternalCommand::AddPlayer { id, name } => {
                self.add_player(Player::new(id, name));
            }
            InternalCommand::RemovePlayer { id } => {
                self.remove_player(id);
            }
        }
    }

    pub fn execute_player_command(&mut self, player_message: PlayerMessage) {
        match player_message.command {
            PlayerCommand::Move { position } => {
                self.move_player(player_message.id, position);
            }
            PlayerCommand::Join { name } => {
                self.execute_internal_command(InternalCommand::AddPlayer {
                    id: player_message.id,
                    name,
                })
            }
        }
    }

    pub fn add_player(&mut self, player: Player) {
        self.send_message_to_player(player.id, MessageToClient::JoinSuccess { id: player.id });
        self.players.push(player);
    }

    pub fn remove_player(&mut self, id: u32) {
        self.players.retain(|player| player.id != id);
        self.send_message_to_player(id, MessageToClient::PlayerEaten { id });
    }

    pub fn move_player(&mut self, id: u32, position: Vector2D) {
        for player in &mut self.players {
            if player.id == id {
                player.move_towards(position);
                return;
            }
        }
    }

    pub fn check_collision(&mut self) {
        for i in 0..self.players.len() {
            for j in 0..self.players.len() {
                let player = &self.players[i];
                let other_player = &self.players[j];

                if player.id != other_player.id {
                    let distance = (player.position - other_player.position).magnitude();
                    if distance < player.radius + other_player.radius {
                        let radius_after_eat = Player::radius_after_eat(player, other_player);
                        if player.radius > other_player.radius {
                            self.players[i].radius = radius_after_eat;
                            self.players[j].radius = 0.0;
                        } else {
                            self.players[j].radius = radius_after_eat;
                            self.players[i].radius = 0.0;
                        }
                    }
                }
            }
        }
    }

    fn mass(radius: f32) -> f32 {
        2.0 * radius.powf(2.0) * std::f32::consts::PI
    }

    fn radius_after_eat(radius1: f32, radius2: f32) -> f32 {
        let combined_mass = GameManager::mass(radius2) + GameManager::mass(radius1);
        (combined_mass / (2.0 * std::f32::consts::PI)).sqrt()
    }

    pub fn check_food_collision(&mut self) {
        for i in (0..self.players.len()).rev() {
            for j in (0..self.food.len()).rev() {
                let player = &self.players[i];
                let food = &self.food[j];

                let distance = (player.position - food.position).magnitude();
                if distance < player.radius + food.radius {
                    let combined = GameManager::radius_after_eat(player.radius, food.radius);
                    self.players[i].radius = combined;
                    self.food.remove(j);
                }
            }
        }
    }

    pub fn update(&mut self) {
        self.check_collision();
        self.check_food_collision();
        self.remove_dead_players();
        self.check_food();
    }

    fn check_food(&mut self) {
        // Check if there are enough food
        if self.food.len() < 50 {
            let difference: u32 = (50 - self.food.len()) as u32;
            let extra_food = GameManager::generate_food(difference);
            self.food.extend(extra_food);
        }
    }

    pub fn remove_dead_players(&self) {
        for index in 0..self.players.len() {
            let player = &self.players[index];
            if player.radius <= 0.01 {
                let command_tx = self.command_tx.clone();
                let id = player.id;
                tokio::spawn(async move {
                    if let Err(error) = command_tx
                        .send(Command::InternalCommand(InternalCommand::RemovePlayer {
                            id,
                        }))
                        .await
                    {
                        println!("Error sending remove player command: {}", error);
                    }
                });
            }
        }
    }
}
