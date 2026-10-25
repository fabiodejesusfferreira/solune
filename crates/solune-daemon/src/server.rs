//! Servidor do socket local: um cliente por tarefa, todos falando com o ator.

use std::sync::Arc;

use solune_ipc::transport::{Listener, ServerConnection};
use solune_ipc::{Command, Event, Outcome, ServerMessage};
use tokio::sync::{Notify, broadcast, mpsc, oneshot};

use crate::player::Msg;

/// Aceita clientes até o processo terminar.
pub async fn serve(
    listener: Listener,
    player: mpsc::UnboundedSender<Msg>,
    events: broadcast::Sender<Event>,
    shutdown: Arc<Notify>,
) {
    let version = env!("CARGO_PKG_VERSION");
    loop {
        match listener.accept(version).await {
            Ok(connection) => {
                tracing::debug!("cliente conectado");
                tokio::spawn(client(
                    connection,
                    player.clone(),
                    events.subscribe(),
                    Arc::clone(&shutdown),
                ));
            }
            // Um cliente com defeito não derruba o servidor.
            Err(e) => tracing::warn!("cliente recusado: {e}"),
        }
    }
}

async fn client(
    connection: ServerConnection,
    player: mpsc::UnboundedSender<Msg>,
    mut events: broadcast::Receiver<Event>,
    shutdown: Arc<Notify>,
) {
    let (mut reader, mut writer) = connection.into_split();
    loop {
        tokio::select! {
            request = reader.next_request() => {
                let (id, command) = match request {
                    Ok(Some(request)) => request,
                    Ok(None) => break,
                    Err(e) => {
                        tracing::warn!("mensagem inválida do cliente: {e}");
                        break;
                    }
                };
                let is_shutdown = command == Command::Shutdown;
                let outcome = ask(&player, command).await;
                let done = matches!(outcome, Outcome::Ok(_));
                if writer.send(&ServerMessage::Response { id, outcome }).await.is_err() {
                    break;
                }
                // Encerra só depois de a resposta sair, para o cliente saber.
                if is_shutdown && done {
                    shutdown.notify_one();
                }
            }
            event = events.recv() => {
                let event = match event {
                    Ok(event) => event,
                    // Cliente lento perdeu eventos; o próximo status corrige.
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => break,
                };
                if writer.send(&ServerMessage::Event { event }).await.is_err() {
                    break;
                }
            }
        }
    }
    tracing::debug!("cliente desconectado");
}

async fn ask(player: &mpsc::UnboundedSender<Msg>, command: Command) -> Outcome {
    let (tx, rx) = oneshot::channel();
    if player.send(Msg::Command(command, tx)).is_err() {
        return Outcome::Error(solune_ipc::ErrorCode::Internal);
    }
    rx.await
        .unwrap_or(Outcome::Error(solune_ipc::ErrorCode::Internal))
}
