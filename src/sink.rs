//! The audio output on Linux: ALSA (through PipeWire's or PulseAudio's ALSA plugin), opened
//! when playback starts and closed when it stops, as librespot's own ALSA sink does, so the
//! device costs nothing while nothing plays.
//!
//! Deck has its own for one difference: how it stops (a pause, the end of the queue).
//! librespot's sink lets the device play out what it holds with `drain`, on its player
//! thread, and PipeWire's ALSA plugin 1.6.10 never returns from `drain`: the player thread
//! hung at the first pause, so Play gave no sound, and quitting hung on joining it. Here
//! stopping waits as long as the audio the device still holds (`delay`, half a second at
//! most) and closes it without `drain`, on a thread of its own: a device that doesn't
//! close within [`STOP_LIMIT`] is left behind, and the next start opens a new one.
//!
//! Sound that stops or starts in the middle of a wave clicks: stopping ends on a short
//! fade to silence, and the first moment after a start fades in ([`FADE`]).

use std::{sync::mpsc, time::Duration};

use alsa::{
    Direction, ValueOr,
    pcm::{Access, Format, Frames, HwParams, PCM, State},
};
use librespot_playback::{
    NUM_CHANNELS, SAMPLE_RATE,
    audio_backend::{Sink, SinkError, SinkResult},
    config::AudioFormat,
    convert::Converter,
    decoder::AudioPacket,
};

/// The ALSA device: the sound server's default output.
const DEVICE: &str = "default";
/// How long stopping waits for the device to play out its buffer (half a second of
/// audio at most, [`BUFFER`]) and close, before leaving it.
const STOP_LIMIT: Duration = Duration::from_secs(2);
/// Half a second of audio in four periods, as librespot's sink asks for: large enough to
/// keep the CPU idle between writes.
const BUFFER: Frames = (SAMPLE_RATE / 2) as Frames;
const PERIOD: Frames = BUFFER / 4;
/// The sound server holds some audio of its own past the device's buffer (PipeWire: a
/// quantum or two): stopping waits this much longer, so the fade-out is heard to the end.
const SERVER_LATENCY: Duration = Duration::from_millis(150);
/// The fades at a stop and a start, in frames: 5 ms, too short to hear as a fade.
const FADE: usize = SAMPLE_RATE as usize / 200;

const CHANNELS: usize = NUM_CHANNELS as usize;

/// 16-bit samples, the format Deck asks librespot for (`AudioFormat::default()`).
pub struct AlsaSink {
    pcm: Option<PCM>,
    /// The last frame written, where the fade-out starts.
    last: [i16; CHANNELS],
    /// Frames of the fade-in written since the device opened.
    faded_in: usize,
}

/// librespot's `SinkBuilder`: the device opens on the first `start`.
pub fn open(_device: Option<String>, _format: AudioFormat) -> Box<dyn Sink> {
    Box::new(AlsaSink {
        pcm: None,
        last: [0; CHANNELS],
        faded_in: 0,
    })
}

impl Sink for AlsaSink {
    fn start(&mut self) -> SinkResult<()> {
        if self.pcm.is_none() {
            let pcm = open_device()
                .map_err(|e| SinkError::ConnectionRefused(format!("ALSA {DEVICE}: {e}")))?;
            self.pcm = Some(pcm);
            self.faded_in = 0;
        }
        Ok(())
    }

    /// Never an error: librespot ends the whole program when stopping fails.
    fn stop(&mut self) -> SinkResult<()> {
        let Some(pcm) = self.pcm.take() else {
            return Ok(());
        };
        let _ = write_s16(&pcm, &fade_out(self.last));
        self.last = [0; CHANNELS];
        let (done, closed) = mpsc::channel();
        std::thread::spawn(move || {
            play_out(&pcm);
            drop(pcm);
            let _ = done.send(());
        });
        if closed.recv_timeout(STOP_LIMIT).is_err() {
            log::warn!("ALSA {DEVICE} did not close in {STOP_LIMIT:?}: left behind");
        }
        Ok(())
    }

    fn write(&mut self, packet: AudioPacket, converter: &mut Converter) -> SinkResult<()> {
        let Some(pcm) = &self.pcm else {
            return Err(SinkError::NotConnected(format!(
                "ALSA {DEVICE} is not open"
            )));
        };
        let written = match &packet {
            AudioPacket::Samples(samples) => {
                let mut samples = converter.f64_to_s16(samples);
                self.faded_in = fade_in(&mut samples, self.faded_in);
                if let Some(last) = samples.rchunks_exact(CHANNELS).next() {
                    self.last.copy_from_slice(last);
                }
                write_s16(pcm, &samples)
            }
            // Only librespot's passthrough mode (Ogg as it is) gives raw bytes, and Deck
            // doesn't use it.
            AudioPacket::Raw(_) => {
                return Err(SinkError::OnWrite("raw packets are not supported".into()));
            }
        };
        // A failed write that can't be recovered closes the device: librespot pauses, and
        // the next start opens it again.
        written.map_err(|e| {
            self.pcm = None;
            SinkError::OnWrite(format!("ALSA {DEVICE}: {e}"))
        })
    }
}

/// Lets the device play what it still holds, then stops it. `drain` would do the same, but
/// PipeWire's ALSA plugin never returns from it (see the module's text).
fn play_out(pcm: &PCM) {
    // Less than the start threshold written (a very short play): it hasn't started yet.
    if pcm.state() == State::Prepared {
        let _ = pcm.start();
    }
    if let Ok(frames) = pcm.delay()
        && frames > 0
    {
        let held = Duration::from_secs_f64(frames as f64 / f64::from(SAMPLE_RATE));
        let most = Duration::from_secs_f64(BUFFER as f64 / f64::from(SAMPLE_RATE));
        std::thread::sleep(held.min(most) + SERVER_LATENCY);
    }
    let _ = pcm.drop();
}

/// Fades in the start of `samples`, `done` frames of the fade being written already;
/// returns how many are now.
fn fade_in(samples: &mut [i16], done: usize) -> usize {
    let mut frame = done;
    for chunk in samples.as_chunks_mut::<CHANNELS>().0 {
        if frame >= FADE {
            break;
        }
        let gain = frame as f32 / FADE as f32;
        for sample in chunk {
            *sample = (f32::from(*sample) * gain) as i16;
        }
        frame += 1;
    }
    frame
}

/// From the last frame written down to silence.
fn fade_out(last: [i16; CHANNELS]) -> Vec<i16> {
    (1..=FADE)
        .flat_map(|frame| {
            let gain = 1.0 - frame as f32 / FADE as f32;
            last.map(|sample| (f32::from(sample) * gain) as i16)
        })
        .collect()
}

/// Writes all of `samples`, recovering from an underrun (the device ran dry) on the way.
fn write_s16(pcm: &PCM, samples: &[i16]) -> alsa::Result<()> {
    let io = pcm.io_i16()?;
    let channels = usize::from(NUM_CHANNELS);
    let mut rest = samples;
    while !rest.is_empty() {
        match io.writei(rest) {
            Ok(frames) => rest = &rest[(frames * channels).min(rest.len())..],
            Err(e) => pcm.try_recover(e, true)?,
        }
    }
    Ok(())
}

fn open_device() -> alsa::Result<PCM> {
    let pcm = PCM::new(DEVICE, Direction::Playback, false)?;
    {
        let hwp = HwParams::any(&pcm)?;
        hwp.set_access(Access::RWInterleaved)?;
        hwp.set_format(Format::s16())?;
        hwp.set_rate(SAMPLE_RATE, ValueOr::Nearest)?;
        hwp.set_channels(u32::from(NUM_CHANNELS))?;
        // A device that won't take these sizes keeps its own: a failed set may leave the
        // parameters unusable, so the copy from before is used then.
        let defaults = hwp.clone();
        let sized = hwp
            .set_buffer_size_near(BUFFER)
            .and_then(|_| hwp.set_period_size_near(PERIOD, ValueOr::Nearest));
        if sized.is_ok() {
            pcm.hw_params(&hwp)?;
        } else {
            pcm.hw_params(&defaults)?;
        }
    }
    {
        let hwp = pcm.hw_params_current()?;
        let (buffer, period) = (hwp.get_buffer_size()?, hwp.get_period_size()?);
        // Starts once the buffer is nearly full, so the first periods don't run dry.
        let swp = pcm.sw_params_current()?;
        swp.set_start_threshold(buffer - period)?;
        pcm.sw_params(&swp)?;
    }
    Ok(pcm)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_start_fades_in_over_several_writes() {
        let mut first = vec![1000; 100 * CHANNELS];
        let done = fade_in(&mut first, 0);
        assert_eq!(done, 100);
        assert_eq!(first[..CHANNELS], [0, 0]);
        assert!(first[first.len() - 1] < 1000);

        let mut rest = vec![1000; FADE * CHANNELS];
        assert_eq!(fade_in(&mut rest, done), FADE);
        assert!(rest[0] > first[first.len() - 1]);
        assert_eq!(rest[rest.len() - 1], 1000);
    }

    #[test]
    fn a_stop_fades_from_the_last_frame_to_silence() {
        let fade = fade_out([1000, -1000]);
        assert_eq!(fade.len(), FADE * CHANNELS);
        assert!(fade[0] > 990 && fade[1] < -990);
        assert_eq!(fade[fade.len() - CHANNELS..], [0, 0]);
    }
}
