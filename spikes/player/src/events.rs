//! Thread de eventos do libmpv para as superfícies com render API.
//!
//! O render.h pede que a thread de render não chame a API comum do libmpv
//! (comandos, propriedades, eventos): ela pode esperar pelo próprio render e
//! travar os quadros. Por isso o carregamento do arquivo e os eventos ficam
//! numa thread separada, que avisa o laço da superfície quando acabar.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use smithay_client_toolkit::reexports::calloop::channel::{self, Channel, Sender};

use crate::mpv::{self, Mpv};
use crate::{Flow, past, poll};

/// Resultado da reprodução: `Ok` no fim normal, `Err` com a mensagem.
pub type Outcome = Result<(), String>;

#[derive(Debug)]
pub struct Events {
    stop: Arc<AtomicBool>,
    ready: Option<mpsc::Sender<()>>,
    join: Option<JoinHandle<()>>,
}

impl Events {
    /// Inicia a thread. O arquivo só é carregado depois de `ready`.
    pub fn spawn(
        player: Arc<Mpv>,
        video: &str,
        deadline: Option<Instant>,
    ) -> (Self, Channel<Outcome>) {
        let stop = Arc::new(AtomicBool::new(false));
        let (ready_tx, ready_rx) = mpsc::channel();
        let (outcome_tx, outcome_rx) = channel::channel();
        let join = thread::spawn({
            let stop = Arc::clone(&stop);
            let video = video.to_owned();
            move || run(&player, &video, deadline, &stop, &ready_rx, &outcome_tx)
        });
        let events = Self {
            stop,
            ready: Some(ready_tx),
            join: Some(join),
        };
        (events, outcome_rx)
    }

    /// Avisa que o contexto de render existe e o arquivo pode ser carregado.
    pub fn ready(&mut self) {
        if let Some(ready) = self.ready.take() {
            let _ = ready.send(());
        }
    }

    /// Pede para a thread parar e espera por ela.
    pub fn shutdown(mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

fn run(
    player: &Mpv,
    video: &str,
    deadline: Option<Instant>,
    stop: &AtomicBool,
    ready: &mpsc::Receiver<()>,
    outcome: &Sender<Outcome>,
) {
    let play = || -> Result<(), mpv::Error> {
        loop {
            if stop.load(Ordering::Relaxed) || past(deadline) {
                return Ok(());
            }
            match ready.recv_timeout(Duration::from_millis(100)) {
                Ok(()) => break,
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(()),
            }
        }
        player.command(&["loadfile", video])?;
        let mut reported = false;
        while !stop.load(Ordering::Relaxed) && !past(deadline) {
            if poll(player, video, 0.25, &mut reported)? == Flow::Stop {
                break;
            }
        }
        Ok(())
    };
    let _ = outcome.send(play().map_err(|e| e.to_string()));
}
