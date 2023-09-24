use std::{
  error::Error,
  ops::{Deref, DerefMut},
  sync::{
    mpsc::{Receiver, RecvError, Sender},
    Arc, Condvar, Mutex,
  },
  thread::JoinHandle,
  time::Duration,
};

use crossterm::{
  cursor,
  event::{Event as CrosstermEvent, KeyEvent, KeyEventKind, MouseEvent},
  terminal::{EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::backend::CrosstermBackend as Backend;
use serde::{Deserialize, Serialize};

pub type Frame<'a> = ratatui::Frame<'a, Backend<std::io::Stderr>>;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Event {
  Init,
  Quit,
  Error,
  Closed,
  Tick,
  Render,
  FocusGained,
  FocusLost,
  Paste(String),
  Key(KeyEvent),
  Mouse(MouseEvent),
  Resize(u16, u16),
}

pub struct Tui {
  pub terminal: ratatui::Terminal<Backend<std::io::Stderr>>,
  pub task: JoinHandle<()>,
  pub event_rx: Receiver<Event>,
  pub event_tx: Sender<Event>,
  pub frame_rate: f64,
  pub tick_rate: f64,
  pub should_cancel: Arc<(Mutex<bool>, Condvar)>,
}

impl Tui {
  pub fn new() -> std::io::Result<Self> {
    let tick_rate = 4.0;
    let frame_rate = 60.0;
    let terminal = ratatui::Terminal::new(Backend::new(std::io::stderr()))?;
    let (event_tx, event_rx) = std::sync::mpsc::channel();
    let task = std::thread::spawn(|| {});
    let should_cancel = Arc::new((Mutex::new(false), Condvar::new()));
    Ok(Self { terminal, task, event_rx, event_tx, frame_rate, tick_rate, should_cancel })
  }

  pub fn tick_rate(mut self, tick_rate: f64) -> Self {
    self.tick_rate = tick_rate;
    self
  }

  pub fn frame_rate(mut self, frame_rate: f64) -> Self {
    self.frame_rate = frame_rate;
    self
  }

  pub fn start(&mut self) {
    let tick_delay = std::time::Duration::from_secs_f64(1.0 / self.tick_rate);
    let render_delay = std::time::Duration::from_secs_f64(1.0 / self.frame_rate);
    let _event_tx = self.event_tx.clone();
    _event_tx.send(Event::Init).unwrap();
    std::thread::spawn(move || {
      _event_tx.send(Event::Render).unwrap();
      std::thread::sleep(render_delay);
    });
    let _event_tx = self.event_tx.clone();
    std::thread::spawn(move || {
      _event_tx.send(Event::Tick).unwrap();
      std::thread::sleep(tick_delay);
    });
    let should_cancel = self.should_cancel.clone();
    let _event_tx = self.event_tx.clone();
    self.task = std::thread::spawn(move || {
      loop {
        {
          let (cancel, _) = &*should_cancel;
          if *cancel.lock().unwrap() {
            break;
          }
        }
        if crossterm::event::poll(Duration::from_millis(1_000)).unwrap() {
          let event = crossterm::event::read().unwrap();
          match event {
            CrosstermEvent::Key(key) => {
              if key.kind == KeyEventKind::Press {
                _event_tx.send(Event::Key(key)).unwrap();
              }
            },
            CrosstermEvent::Mouse(mouse) => {
              _event_tx.send(Event::Mouse(mouse)).unwrap();
            },
            CrosstermEvent::Resize(x, y) => {
              _event_tx.send(Event::Resize(x, y)).unwrap();
            },
            CrosstermEvent::FocusLost => {
              _event_tx.send(Event::FocusLost).unwrap();
            },
            CrosstermEvent::FocusGained => {
              _event_tx.send(Event::FocusGained).unwrap();
            },
            CrosstermEvent::Paste(s) => {
              _event_tx.send(Event::Paste(s)).unwrap();
            },
          }
        }
      }
    });
  }

  pub fn stop(&self) {
    let mut counter = 0;
    while !self.task.is_finished() {
      std::thread::sleep(Duration::from_millis(10));
      counter += 1;
      if counter > 100 {
        log::error!("Failed to abort task in 1 seconds for unknown reason");
        break;
      }
    }
  }

  pub fn enter(&mut self) -> std::io::Result<()> {
    crossterm::terminal::enable_raw_mode()?;
    crossterm::execute!(std::io::stderr(), EnterAlternateScreen, cursor::Hide)?;
    self.start();
    Ok(())
  }

  pub fn exit(&mut self) -> std::io::Result<()> {
    self.stop();
    if crossterm::terminal::is_raw_mode_enabled()? {
      self.flush()?;
      crossterm::execute!(std::io::stderr(), LeaveAlternateScreen, cursor::Show)?;
      crossterm::terminal::disable_raw_mode()?;
    }
    Ok(())
  }

  pub fn cancel(&self) {
    let (lock, cvar) = &*self.should_cancel;
    let mut cancel = lock.lock().unwrap();
    *cancel = true;
    cvar.notify_one();
  }

  pub fn suspend(&mut self) -> Result<(), Box<dyn Error>> {
    self.exit()?;
    #[cfg(not(windows))]
    signal_hook::low_level::raise(signal_hook::consts::signal::SIGTSTP)?;
    Ok(())
  }

  pub fn resume(&mut self) -> std::io::Result<()> {
    self.enter()?;
    Ok(())
  }

  pub fn next(&mut self) -> Result<Event, RecvError> {
    self.event_rx.recv()
  }
}

impl Deref for Tui {
  type Target = ratatui::Terminal<Backend<std::io::Stderr>>;

  fn deref(&self) -> &Self::Target {
    &self.terminal
  }
}

impl DerefMut for Tui {
  fn deref_mut(&mut self) -> &mut Self::Target {
    &mut self.terminal
  }
}

impl Drop for Tui {
  fn drop(&mut self) {
    self.exit().unwrap();
  }
}
