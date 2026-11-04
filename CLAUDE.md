# CLAUDE.md

Deck is a terminal Spotify player for macOS written in Rust: ratatui for the UI,
librespot for sign-in and playback, Spotify's Web API for search and likes. See
`README.md` for what it does from the user's point of view.

## Build and test

```sh
cargo build --locked
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
cargo fmt --check
```

All four must pass before a commit.

- Always pass `--locked`. `Cargo.lock` pins `vergen` to 9.0.6 because librespot 0.8 does
  not build with newer versions.
- **Never run `cargo update`** (not even for a single crate without checking the result):
  it breaks the build. Add dependencies with an explicit version and check that
  `Cargo.lock` only gained what you added.
- Tests must not touch the network or the real `~/.config`, `~/.cache` or
  `~/.local/state`. Use `tempfile` for files and fake responses (see
  `tests/fixtures/`) for Spotify. Tests that need a real Spotify account are `#[ignore]`d.
- Spotify blocks the whole app for hours after too many Web API requests (429). Do not
  write loops or experiments that call the Web API repeatedly.

## Releases

A release is `version` in `Cargo.toml` and a tag `vX.Y.Z`. Nightdrive installs Deck as the
`deck` package from its COPR (`@nightdrive/nightdrive`), so once the tag is pushed, bump
`deck/deck.spec` in nightdrive-packages (`Version`, `Release: 1`, a `%changelog` entry)
and push that: COPR builds the new package, and Nightdrive's System update installs it.

## Architecture

- **`app`**: all state and logic. `State::update(Event, Instant) -> Vec<Action>` is pure:
  it changes the state and returns what should happen (play, search, save the shelf…),
  but does no IO itself. Events are key presses, player events, results of earlier
  actions and ticks. Nearly all behaviour is tested through `update`.
  `src/app.rs` holds the data model, `State::new` and `update`; `State`'s methods are
  split by topic into `src/app/` (`event`, `view`, `keys`, `open`, `play`, `station` for
  the genre radio, `playing` for player events, media keys and `deck ctl`, `results`,
  `likes`, `saved`, `query` for what `ui` reads). A method another part of `app` calls is
  `pub(super)`. Its tests are in `src/app/tests/`, one file per topic, with the shared
  fixtures in `src/app/tests.rs`.
- **`main`** and its helpers: the event loop and all IO. `src/main.rs` is the module list,
  `main` and the subcommands' dispatch (`deck taste`, `deck curate`, `deck curate submit`,
  `deck curate genres`, `deck genre …`, `deck status --json`, `deck ctl`, and the
  internal `deck now-playing`);
  `cli` parses the command line (`Mode`, the usage). `event_loop` signs in, reads keys and
  player events, calls `State::update` and draws; `deck` (`Deck`) carries out each
  `Action` (often in a tokio task that sends the result back as an `Event`); `logging` is
  the log file and stderr sent to it while the UI is open.
- **`ui`**: draws the state with ratatui. No logic: decisions belong in `app`. `src/ui.rs`
  is the layout, the top rows and the colours; the views are in `src/ui/home.rs` (shelf,
  Genres, Curated) and `src/ui/views.rs`, the shared list and text helpers in
  `src/ui/list.rs`, the key help rows in `src/ui/key_help.rs`, and the tests in
  `src/ui/tests/` as in `app`.

Other modules (a module split into parts has them in `src/<module>/`, its public paths
re-exported, and its tests in `src/<module>/tests.rs` or, by topic, `tests/`):

| Module | What it does |
|---|---|
| `player` | Sign-in and playback with librespot (`Player` takes commands, sends events), and the Spotify session shared with `radio` and `catalog` (`SharedSession`) |
| `catalog` | Spotify Web API: search, artist albums, album tracks, Liked Songs. `src/catalog/`: `search`, `auth` (the own app's sign-in and saved token), `memo` (responses kept in memory), `api` (the JSON types) |
| `radio` | Genre radio pages from Spotify's radio stations (apollo) |
| `genres` | The built-in genre catalog (`genres/genres.json`, compiled in); favourites and the user's own genres in `src/genres/` (`favorites`, `my_genres`) |
| `genre_cli` | `deck genre add / remove / list / prompt`; the seed check in `src/genre_cli/check.rs` (Spotify behind a trait), the real Spotify in `live.rs` |
| `shelf` | The album shelf (`~/.config/deck/shelf.json`); the order of artists and albums in `src/shelf/sort.rs` |
| `lists` | The Curated list and its history (tests in `src/lists/tests.rs`) |
| `curate` | `deck curate submit` and its check rules, used by the weekly Curated run; `deck taste` in `src/curate/taste.rs`, the searches with their cache, budget and 429 block in `search.rs`, the albums' rough genres and `deck curate genres` in `genres.rs` |
| `coarse_genre` | A genre list (the curator's, Last.fm tags) reduced to one to three coarse words for a Curated row |
| `curate_run` | `deck curate`: the Curated run, which starts Claude Code with `curate/prompt.md` and logs to `~/.cache/deck/curate.log` |
| `lastfm` | Last.fm API; artists' tags in `src/lastfm/tags.rs` |
| `playback` | Playback state across restarts (`~/.local/state/deck/playback.json`) |
| `now_playing` | macOS Now Playing and media keys, run in a helper process (Linux: MPRIS, the control socket and the state file), started again if it ends. `src/now_playing/`: `control` (`deck ctl`'s commands and their line form), `linux`, `mac` |
| `status` | `deck status --json`: what the running Deck plays, from the state file (Linux) |
| `instance` | One Deck at a time on Linux: the lock `$XDG_RUNTIME_DIR/deck/deck.lock`, held from the start |
| `sink` | The ALSA output on Linux, whose stop gives up on a device that never finishes playing out (PipeWire) |
| `config` | `~/.config/deck/config.toml`, directory paths and the shared OAuth client |
| `rate_limit` | Spotify's 429 block shared by all Deck processes (`~/.cache/deck/rate-limit.json`) |
| `store` | Atomic JSON read and write shared by the modules that keep files |

Curated is built outside the app: `deck curate` runs Claude Code with `curate/prompt.md`
(compiled in), and Claude calls `deck taste` and `deck curate submit`.
`scripts/install-curate.sh` installs a launchd job that runs `deck curate` every Monday.
`genres/prompt.md` is the corresponding prompt for adding genres, printed by
`deck genre prompt`.

## Conventions

- **Everything the user sees is in English**: UI text, errors, command output, README.
- **The UI fits in 100 columns.** Deck is at most 100 columns wide; every help row must
  fit (there is a test for it). If a help row is too long, `[q] quit` is the first to go.
- **Code, comments and doc comments are in English**, like everything else in the repo.
- **Save files atomically** with `store::save_json` (or `store::write_atomic`): it writes a
  uniquely named temp file next to the target and renames it over the target, so a crash
  never leaves half a file and two processes (Deck and `deck curate submit`) never clash.
- **Never overwrite a broken file.** If a user file (shelf, genres, Curated…) cannot be
  parsed, show the error and leave the file alone, so the user can fix it by hand. A
  missing file is not an error: it means empty. `store::read_json` does both. Deck's own
  state and caches (`playback.json`, `webapi.json`, `rate-limit.json`, the search cache)
  are the exception: a broken one is logged and replaced.
- **Reread a user file before changing it** (see `Shelf::add`), so that edits made by
  hand while Deck is open are not overwritten.
- **Secrets are owner-only**: `~/.cache/deck` is 0700 and sign-in files are 0600.
- **Keep files small**: the code is written by Claude, so one module does one thing in
  ~200–400 lines. Past ~600, split off what stands on its own (a submodule, a widget);
  past ~800 it must be split. Plan a new feature in small files from the start.
- Match the surrounding code: small functions, `anyhow` errors with context
  (`cannot open <path>`), and tests next to the code in `mod tests` (in `app` and `ui`,
  in their `tests/` folder by topic).
- Update `README.md` when keys, commands, files or setup change. The key table in the
  README must match `key_help` and the key handling in `app`.
