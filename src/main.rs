use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    response::IntoResponse,
    routing::get,
    Router,
};
use futures::{
    sink::SinkExt,
    stream::{SplitSink, StreamExt},
};

use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{
        atomic::{AtomicU32, Ordering},
        Arc,
    },
};
use tokio::sync::{broadcast, mpsc, Mutex};
use tower_http::cors::CorsLayer;

mod game_manager;
mod player;
mod vector;
use game_manager::{Command, GameManager, InternalCommand, MessageToClient};

use crate::game_manager::{PlayerCommand, PlayerMessage};

#[derive(serde::Deserialize, serde::Serialize, Clone)]
struct User {
    username: String,
    password: String,
}

struct AppState {
    tx_game_manager: mpsc::Sender<Command>,
    rx_game_manager: broadcast::Sender<MessageToClient>,
    id_tracker: Arc<AtomicU32>,
    players_sockets: Arc<Mutex<HashMap<u32, Arc<Mutex<SplitSink<WebSocket, Message>>>>>>,
}

#[tokio::main]
async fn main() {
    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));

    // This channel is used to send messages to all the players
    let (broadcast_channel, _) = broadcast::channel::<MessageToClient>(100);
    let game_manager = GameManager::new(broadcast_channel.clone());
    let command_tx = game_manager.command_tx.clone();

    let app_state = Arc::new(AppState {
        tx_game_manager: command_tx.clone(),
        rx_game_manager: broadcast_channel.clone(),
        id_tracker: Arc::new(AtomicU32::new(0)),
        players_sockets: game_manager.players_sockets.clone(),
    });

    game_manager.start();

    let app = Router::new()
        .route("/game", get(websocket_handler))
        .with_state(app_state)
        .layer(CorsLayer::very_permissive());

    axum::Server::bind(&addr)
        .serve(app.into_make_service())
        .await
        .unwrap();
}

async fn websocket_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    ws.on_upgrade(|socket| websocket_connection(socket, state))
}

async fn websocket_connection(stream: WebSocket, state: Arc<AppState>) {
    let id = state.id_tracker.fetch_add(1, Ordering::SeqCst);
    let (socket_sender, mut socket_receiver) = stream.split();

    let tx_game_manager = state.tx_game_manager.clone();
    let mut rx_game_manager = state.rx_game_manager.subscribe();

    // Adds the socket to the list of sockets so that the game manager can send messages directly to a player
    let socket_sender = Arc::new(Mutex::new(socket_sender));
    let mut players_sockets = state.players_sockets.lock().await;
    players_sockets.insert(id, socket_sender.clone());

    // Recieves messages from the game manager and sends them to the client
    tokio::spawn(async move {
        while let Ok(msg) = rx_game_manager.recv().await {
            let msg_string = serde_json::to_string::<MessageToClient>(&msg);
            match msg_string {
                Ok(msg_string) => {
                    let sender = socket_sender.clone();
                    let mut sender = sender.lock().await;

                    if let Err(e) = sender.send(Message::Text(msg_string.clone())).await {
                        println!("Error sending message to client {}", e);
                        break;
                    }
                }
                Err(e) => println!("Error serializing message: {}", e),
            }
        }
    });

    // Recieves messages from the client and sends them to the game manager
    tokio::spawn(async move {
        while let Some(Ok(Message::Text(text))) = socket_receiver.next().await {
            println!("Received message from client: {}", text);

            let command_from_socket = serde_json::from_str::<PlayerCommand>(&text);

            let command_from_socket = match command_from_socket {
                Ok(command_from_socket) => command_from_socket,
                Err(e) => {
                    println!("Error deserializing message: {}", e);
                    continue;
                }
            };

            // Adds the ID to the command so that the game manager knows which player sent the command
            let command_from_socket = PlayerMessage {
                id,
                command: command_from_socket,
            };

            if let Err(e) = tx_game_manager
                .send(Command::PlayerCommand(command_from_socket))
                .await
            {
                println!("Error sending message to game manager: {}", e);
            };
        }

        // Client disconnected
        if let Err(e) = tx_game_manager
            .send(Command::InternalCommand(InternalCommand::RemovePlayer {
                id,
            }))
            .await
        {
            println!("Error sending message to game manager: {}", e);
        };
    });
}
