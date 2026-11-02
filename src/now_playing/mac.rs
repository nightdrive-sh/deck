//! The macOS helper: Now Playing and the media keys through MediaPlayer, on the main
//! thread's event loop.

use std::{
    cell::RefCell,
    io::{BufRead, Write},
    ptr::NonNull,
    time::Instant,
};

use anyhow::{Context, Result};
use block2::RcBlock;
use dispatch2::DispatchQueue;
use objc2::{AllocAnyThread, MainThreadMarker, rc::Retained, runtime::AnyObject};
use objc2_app_kit::NSImage;
use objc2_core_foundation::{CFRunLoop, CGSize};
use objc2_foundation::{NSData, NSMutableDictionary, NSNumber, NSString, NSURL};
use objc2_media_player::{
    MPMediaItemArtwork, MPMediaItemPropertyAlbumTitle, MPMediaItemPropertyArtist,
    MPMediaItemPropertyArtwork, MPMediaItemPropertyPlaybackDuration, MPMediaItemPropertyTitle,
    MPNowPlayingInfoCenter, MPNowPlayingInfoPropertyElapsedPlaybackTime,
    MPNowPlayingInfoPropertyPlaybackRate, MPNowPlayingPlaybackState, MPRemoteCommandCenter,
    MPRemoteCommandEvent, MPRemoteCommandHandlerStatus,
};

use super::{Command, Info, should_download};

thread_local! {
    /// The latest cover image with its URL (main thread only).
    static ARTWORK: RefCell<Option<(String, Retained<MPMediaItemArtwork>)>> =
        const { RefCell::new(None) };
}

/// `deck now-playing`: registers the keys and runs the event loop on the main
/// thread. Info is read in a thread of its own and applied on the main thread.
pub fn serve() -> Result<()> {
    MainThreadMarker::new().context("now-playing must run on the main thread")?;
    register_commands();
    std::thread::spawn(read_updates);
    CFRunLoop::run();
    Ok(())
}

fn register_commands() {
    // SAFETY: MediaPlayer calls on the main thread; the commands live until exit.
    unsafe {
        let center = MPRemoteCommandCenter::sharedCommandCenter();
        let commands = [
            (center.togglePlayPauseCommand(), Command::Toggle),
            (center.playCommand(), Command::Play),
            (center.pauseCommand(), Command::Pause),
            (center.nextTrackCommand(), Command::Next),
            (center.previousTrackCommand(), Command::Previous),
        ];
        for (remote, command) in commands {
            let handler = RcBlock::new(move |_: NonNull<MPRemoteCommandEvent>| {
                send(command);
                MPRemoteCommandHandlerStatus::Success
            });
            remote.setEnabled(true);
            remote.addTargetWithHandler(&handler);
        }
    }
}

/// Writes the key to Deck. If the write fails, Deck is gone.
fn send(command: Command) {
    let mut out = std::io::stdout().lock();
    if writeln!(out, "{}", command.name())
        .and_then(|()| out.flush())
        .is_err()
    {
        std::process::exit(0);
    }
}

/// Reads the info from stdin. A new cover image is downloaded in this thread, so
/// the main thread never waits for the network. A failed download is retried with
/// a later update.
fn read_updates() {
    // URL of the downloaded cover image, and the last failed download.
    let mut cover_url: Option<String> = None;
    let mut failed: Option<(String, Instant)> = None;
    for line in std::io::stdin().lock().lines() {
        let Ok(line) = line else { break };
        let info: Option<Info> = match serde_json::from_str(&line) {
            Ok(info) => info,
            Err(e) => {
                eprintln!("now-playing: invalid update: {e}");
                continue;
            }
        };
        let url = info.as_ref().and_then(|info| info.cover.clone());
        let image = match url {
            Some(url)
                if should_download(&url, cover_url.as_deref(), failed.as_ref(), Instant::now()) =>
            {
                match download(&url) {
                    Some(bytes) => {
                        cover_url = Some(url.clone());
                        failed = None;
                        Some((url, bytes))
                    }
                    None => {
                        failed = Some((url, Instant::now()));
                        None
                    }
                }
            }
            _ => None,
        };
        DispatchQueue::main().exec_async(move || publish(info.as_ref(), image));
    }
    // Deck exited or started a new helper process.
    std::process::exit(0);
}

fn download(url: &str) -> Option<Vec<u8>> {
    let url = NSURL::URLWithString(&NSString::from_str(url))?;
    NSData::dataWithContentsOfURL(&url).map(|data| data.to_vec())
}

fn publish(info: Option<&Info>, image: Option<(String, Vec<u8>)>) {
    // SAFETY: called on the main thread; the keys and values are MediaPlayer types.
    unsafe {
        let center = MPNowPlayingInfoCenter::defaultCenter();
        let Some(info) = info else {
            center.setNowPlayingInfo(None);
            center.setPlaybackState(MPNowPlayingPlaybackState::Stopped);
            return;
        };
        if let Some((url, bytes)) = image {
            ARTWORK.set(artwork(&bytes).map(|artwork| (url, artwork)));
        }
        let dict = NSMutableDictionary::<NSString, AnyObject>::new();
        let text = |key: &NSString, value: &str| {
            dict.insert(key, &*NSString::from_str(value));
        };
        text(MPMediaItemPropertyTitle, &info.title);
        text(MPMediaItemPropertyArtist, &info.artist);
        text(MPMediaItemPropertyAlbumTitle, &info.album);
        let number = |key: &NSString, value: f64| {
            dict.insert(key, &*NSNumber::new_f64(value));
        };
        number(
            MPMediaItemPropertyPlaybackDuration,
            info.duration_ms as f64 / 1000.0,
        );
        number(
            MPNowPlayingInfoPropertyElapsedPlaybackTime,
            info.position_ms as f64 / 1000.0,
        );
        number(
            MPNowPlayingInfoPropertyPlaybackRate,
            if info.playing { 1.0 } else { 0.0 },
        );
        ARTWORK.with_borrow(|artwork| {
            if let Some((url, artwork)) = artwork
                && info.cover.as_ref() == Some(url)
            {
                dict.insert(MPMediaItemPropertyArtwork, &**artwork);
            }
        });
        center.setNowPlayingInfo(Some(&dict));
        center.setPlaybackState(if info.playing {
            MPNowPlayingPlaybackState::Playing
        } else {
            MPNowPlayingPlaybackState::Paused
        });
    }
}

fn artwork(bytes: &[u8]) -> Option<Retained<MPMediaItemArtwork>> {
    let image = NSImage::initWithData(NSImage::alloc(), &NSData::with_bytes(bytes))?;
    let size = image.size();
    let handler = RcBlock::new(move |_: CGSize| NonNull::from(&*image));
    // SAFETY: the block returns a live image, which the block itself keeps alive.
    Some(unsafe {
        MPMediaItemArtwork::initWithBoundsSize_requestHandler(
            MPMediaItemArtwork::alloc(),
            size,
            &handler,
        )
    })
}
