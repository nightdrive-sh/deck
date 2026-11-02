//! The event loop: signs in, loads the files, and feeds key presses, player events,
//! action results and ticks into `State::update`, whose actions [`Deck`] carries out.

use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use ratatui::{
    DefaultTerminal,
    crossterm::event::{self, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
};
use tokio::{
    sync::mpsc::{self, UnboundedReceiver, UnboundedSender},
    time::MissedTickBehavior,
};

use crate::{
    app::{self, Action, Key, State},
    catalog::Catalog,
    config::Config,
    deck::{Deck, curated_list, favorite_ids},
    genres::{self, Favorites, MyGenres},
    history, listened,
    lists::CuratedFiles,
    logging::StderrToLog,
    now_playing, picks,
    playback::PlaybackFile,
    player::{self, Player, SharedSession},
    radio,
    shelf::Shelf,
    ui,
};

/// Tick interval: must be shorter than `app::SEARCH_DEBOUNCE`. The tick also redraws
/// the elapsed time and the progress bar.
const TICK: Duration = Duration::from_millis(100);

/// Signs in before the UI: a failed sign-in ends the program with a clear message.
/// After that, errors are shown in the UI. `headless` (`deck serve`) plays without the UI:
/// a window drives it with `deck ctl` and follows the state file, and errors go to the log.
pub(super) async fn run(headless: bool) -> Result<()> {
    // Two players would fight over the control socket and the saved playback state.
    if now_playing::running() {
        anyhow::bail!("Deck is already running (`deck ctl quit` ends it)");
    }
    if headless && !cfg!(target_os = "linux") {
        anyhow::bail!("deck serve is only available on Linux");
    }
    let config = Config::load()?;
    // The user's own genres before anything reads the genre list; a broken file shows
    // up as an error, and Deck shows only the catalog.
    let my_genres_error = match MyGenres::default_path().and_then(|path| MyGenres::load(&path)) {
        Ok(my) => {
            genres::install(my.genres().to_vec());
            None
        }
        Err(e) => Some(format!("{e:#}")),
    };
    let session = player::connect().await?;
    let shared = SharedSession::new(session.clone());
    let catalog = Catalog::connect(shared.clone(), &config).await?;

    let (shelf, shelf_error) = match Shelf::default_path().and_then(|path| Shelf::load(&path)) {
        Ok(shelf) => (shelf, None),
        Err(e) => (Shelf::unavailable(), Some(format!("{e:#}"))),
    };
    let (favorites, favorites_error) =
        match Favorites::default_path().and_then(|path| Favorites::load(&path)) {
            Ok(favorites) => (favorites, None),
            Err(e) => (Favorites::unavailable(), Some(format!("{e:#}"))),
        };
    let lists = CuratedFiles::default_paths()?;
    let picked = picks::PickedFile::default_path()?;
    let listened = listened::ListenedFile::default_path()?;
    let mut state = State::new(shelf.albums().to_vec())
        .with_picked(picked.load())
        .with_listened(listened.load())
        .with_favorites(favorite_ids(&favorites))
        .with_curated(curated_list(&lists))
        .with_likes(catalog.can_like());
    if let Some(error) = shelf_error.or(favorites_error).or(my_genres_error) {
        state.error = Some(error);
    }

    // Playback state of the previous session. The track loads in the background, so the
    // UI opens at once; a broken file shows up as an error and Deck opens empty.
    let playback = PlaybackFile::default_path()?;
    let restore = match playback.load() {
        Ok(Some(saved)) => state.update(app::Event::Restore(Box::new(saved)), Instant::now()),
        Ok(None) => Vec::new(),
        Err(e) => {
            state.error.get_or_insert(format!("{e:#}"));
            Vec::new()
        }
    };

    let radio = radio::Client::new(shared.clone());
    let (player, player_events) = Player::spawn(shared, session, catalog.clone(), config.bitrate);
    let (results, results_rx) = mpsc::unbounded_channel();
    let media = start_now_playing(&results);
    let mut deck = Deck {
        player,
        catalog,
        radio,
        shelf,
        favorites,
        lists,
        playback,
        picked,
        listened,
        history: history::HistoryFile::default_path()?,
        results,
        media,
    };
    for action in restore {
        deck.perform(action);
    }

    let stderr = StderrToLog::start();
    let outcome = if headless {
        event_loop(None, state, deck, player_events, results_rx).await
    } else {
        let mut terminal = ratatui::init();
        let outcome = event_loop(Some(&mut terminal), state, deck, player_events, results_rx).await;
        ratatui::restore();
        outcome
    };
    drop(stderr);
    outcome
}

/// Feeds key presses, player events, action results and ticks into `State::update`
/// and carries out the actions it returns. Without a terminal (`deck serve`) there are no
/// keys and nothing is drawn, and SIGTERM or Ctrl+C quits as `q` does.
async fn event_loop(
    mut terminal: Option<&mut DefaultTerminal>,
    mut state: State,
    mut deck: Deck,
    mut player_events: UnboundedReceiver<player::Event>,
    mut results: UnboundedReceiver<app::Event>,
) -> Result<()> {
    let mut input = match terminal {
        Some(_) => spawn_input_reader(),
        None => mpsc::unbounded_channel().1,
    };
    let mut quit = quit_signals(terminal.is_none())?;
    let mut tick = tokio::time::interval(TICK);
    tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let mut error = None;

    loop {
        if let Some(terminal) = terminal.as_deref_mut() {
            terminal.draw(|frame| ui::draw(frame, &state, Instant::now()))?;
        } else if state.error != error {
            // Nobody sees the UI's error line: the log keeps it.
            error.clone_from(&state.error);
            if let Some(message) = &error {
                log::warn!("{message}");
            }
        }
        let event = tokio::select! {
            Some(key) = input.recv() => match key {
                Some(key) => app::Event::Key(key),
                // The window was resized: redraw.
                None => continue,
            },
            Some(event) = player_events.recv() => app::Event::Player(event),
            Some(event) = results.recv() => event,
            Some(()) = quit.recv() => app::Event::Quit,
            _ = tick.tick() => app::Event::Tick,
        };
        let now = Instant::now();
        for action in state.update(event, now) {
            if action == Action::Quit {
                deck.save_playback(state.playback(now));
                return Ok(());
            }
            deck.perform(action);
        }
        if let Some(played) = state.picked_changed()
            && let Err(e) = deck.picked.save(played)
        {
            log::warn!("{e:#}");
        }
        if let Some(listened) = state.listened_changed()
            && let Err(e) = deck.listened.save(listened)
        {
            log::warn!("{e:#}");
        }
        if state.playback_changed() {
            deck.save_playback(state.playback(now));
        }
        if let Some(media) = &mut deck.media {
            media.update(state.now_playing_info(now), now);
        }
    }
}

/// SIGTERM and Ctrl+C for `deck serve`, which has no `q` key; the TUI gets neither (`None`).
fn quit_signals(headless: bool) -> Result<UnboundedReceiver<()>> {
    let (sender, receiver) = mpsc::unbounded_channel();
    if !headless {
        return Ok(receiver);
    }
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        let mut term = signal(SignalKind::terminate()).context("cannot listen for SIGTERM")?;
        let mut int = signal(SignalKind::interrupt()).context("cannot listen for SIGINT")?;
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = term.recv() => {}
                    _ = int.recv() => {}
                }
                if sender.send(()).is_err() {
                    break;
                }
            }
        });
    }
    Ok(receiver)
}

/// macOS Now Playing and media keys. If the helper process does not start, Deck works
/// without them.
fn start_now_playing(results: &UnboundedSender<app::Event>) -> Option<now_playing::Session> {
    if !cfg!(any(target_os = "macos", target_os = "linux")) {
        return None;
    }
    let results = results.clone();
    now_playing::Session::start(move |control| {
        let _ = results.send(match control {
            now_playing::Control::Media(command) => app::Event::Media(command),
            now_playing::Control::PlayAlbum(uri) => app::Event::PlayAlbum(uri),
            now_playing::Control::PlaySong { track, album } => {
                app::Event::PlaySong { track, album }
            }
            now_playing::Control::Search(query) => app::Event::Search(query),
            now_playing::Control::QueueAlbum(album) => {
                app::Event::QueueRemote { album, track: None }
            }
            now_playing::Control::QueueSong { track, album } => app::Event::QueueRemote {
                album,
                track: Some(track),
            },
            now_playing::Control::Unqueue(index) => app::Event::Unqueue(index),
            now_playing::Control::ClearQueue => app::Event::ClearQueue,
            now_playing::Control::Pick(from) => app::Event::Pick(from),
            now_playing::Control::ShelfAdd(uri) => app::Event::Shelve { uri, on: true },
            now_playing::Control::ShelfRemove(uri) => app::Event::Shelve { uri, on: false },
            now_playing::Control::Seek(ms) => app::Event::Seek(Duration::from_millis(ms)),
            now_playing::Control::Radio(id) => app::Event::Radio(id),
            now_playing::Control::FavoriteAdd(id) => app::Event::Favorite { id, on: true },
            now_playing::Control::FavoriteRemove(id) => app::Event::Favorite { id, on: false },
            now_playing::Control::RejectCurated(uri) => app::Event::RejectCurated(uri),
            now_playing::Control::Quit => app::Event::Quit,
            // The same as the start's own restore: paused at the saved position, with the queue
            now_playing::Control::Save(path) => app::Event::SavePlayback(path),
            now_playing::Control::Restore(path) => match PlaybackFile::at(path).load() {
                Ok(Some(saved)) => app::Event::RestoreCopy(Box::new(saved)),
                Ok(None) => return log::warn!("deck ctl restore: no such file"),
                Err(e) => return log::warn!("deck ctl restore: {e:#}"),
            },
        });
    })
    .inspect_err(|e| log::warn!("{e:#}"))
    .ok()
}
/// Reads terminal input on its own thread. `None` means that the window was
/// resized.
fn spawn_input_reader() -> UnboundedReceiver<Option<Key>> {
    let (sender, receiver) = mpsc::unbounded_channel();
    std::thread::spawn(move || {
        while let Ok(input) = event::read() {
            let message = match input {
                event::Event::Key(key_event) => match key(key_event) {
                    Some(key) => Some(key),
                    None => continue,
                },
                event::Event::Resize(..) => None,
                _ => continue,
            };
            if sender.send(message).is_err() {
                break;
            }
        }
    });
    receiver
}

/// Converts a crossterm key into an `app::Key`. Ctrl letters are lowercase.
fn key(event: KeyEvent) -> Option<Key> {
    if event.kind == KeyEventKind::Release {
        return None;
    }
    let key = match event.code {
        KeyCode::Char(c) if event.modifiers.contains(KeyModifiers::CONTROL) => {
            Key::Ctrl(c.to_ascii_lowercase())
        }
        KeyCode::Char(c) => Key::Char(c),
        KeyCode::Up => Key::Up,
        KeyCode::Down => Key::Down,
        KeyCode::Left => Key::Left,
        KeyCode::Right => Key::Right,
        KeyCode::Enter => Key::Enter,
        KeyCode::Esc => Key::Esc,
        KeyCode::Backspace => Key::Backspace,
        _ => return None,
    };
    Some(key)
}
#[cfg(test)]
mod tests {
    use super::*;

    fn press(code: KeyCode, modifiers: KeyModifiers) -> Option<Key> {
        key(KeyEvent::new(code, modifiers))
    }
    #[test]
    fn keys_convert_to_app_keys() {
        assert_eq!(
            press(KeyCode::Char('f'), KeyModifiers::CONTROL),
            Some(Key::Ctrl('f'))
        );
        assert_eq!(
            press(
                KeyCode::Char('A'),
                KeyModifiers::CONTROL | KeyModifiers::SHIFT
            ),
            Some(Key::Ctrl('a'))
        );
        assert_eq!(
            press(KeyCode::Char('Ä'), KeyModifiers::SHIFT),
            Some(Key::Char('Ä'))
        );
        assert_eq!(
            press(KeyCode::Char(' '), KeyModifiers::NONE),
            Some(Key::Char(' '))
        );
        assert_eq!(press(KeyCode::Enter, KeyModifiers::NONE), Some(Key::Enter));
        assert_eq!(press(KeyCode::F(1), KeyModifiers::NONE), None);

        let mut release = KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE);
        release.kind = KeyEventKind::Release;
        assert_eq!(key(release), None);
    }
}
