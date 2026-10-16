//! How long a sound plays: a WAV's samples, or an MP3's frames, each of 1152 samples
//! (MPEG-1 layer III) or 576 (MPEG-2 and 2.5), over its sample rate. The original
//! decodes a sound whole before playing it (`S_LoadSound`), so this is the time its
//! mixer plays it for, which is when a script waiting on a voice line goes on
//! (`G_CheckTasksCompleted`: the entity's voice no longer plays).

/// The files a sound named `name` is read from, in the order the original tries
/// them (`S_LoadSound`): its name without extension, as a WAV, then as an MP3.
pub fn files(name: &str) -> [String; 2] {
    let stem = match name.rsplit_once('.') {
        Some((stem, ext)) if !ext.contains('/') => stem,
        _ => name,
    };
    [format!("{stem}.wav"), format!("{stem}.mp3")]
}

/// How long the sound in `bytes` plays (ms), read as a WAV or an MP3 by its first
/// bytes; `None` if it is neither or has no samples.
pub fn length_ms(bytes: &[u8]) -> Option<f32> {
    if bytes.starts_with(b"RIFF") {
        wav_length(bytes)
    } else {
        mp3_length(bytes)
    }
}

fn u16_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u16::from_le_bytes(b.get(at..at + 2)?.try_into().ok()?) as u32)
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

/// A WAV's data chunk over its format's bytes a second.
fn wav_length(b: &[u8]) -> Option<f32> {
    let (mut at, mut rate, mut align) = (12, None, None);
    while at + 8 <= b.len() {
        let size = u32_at(b, at + 4)? as usize;
        match &b[at..at + 4] {
            b"fmt " => {
                rate = u32_at(b, at + 12);
                align = u16_at(b, at + 20);
            }
            b"data" => {
                let size = size.min(b.len() - at - 8);
                let (rate, align) = (rate?, align?);
                return (rate > 0 && align > 0 && size > 0)
                    .then(|| size as f32 / align as f32 / rate as f32 * 1000.0);
            }
            _ => {}
        }
        at += 8 + size + size % 2;
    }
    None
}

/// An MP3's layer III frames: after any ID3v2 tag, frame after frame while their
/// headers hold; a first frame holding a Xing or Info header carries no sound.
fn mp3_length(b: &[u8]) -> Option<f32> {
    let mut at = 0;
    if b.starts_with(b"ID3") && b.len() >= 10 {
        let size = b[6..10]
            .iter()
            .fold(0usize, |s, &x| (s << 7) | (x & 0x7f) as usize);
        let footer = if b[5] & 0x10 != 0 { 10 } else { 0 };
        at = 10 + size + footer;
    }
    let (mut samples, mut rate, mut first) = (0u64, 0u32, true);
    while let Some((length, frame_samples, frame_rate)) = b.get(at..at + 4).and_then(header) {
        let tagged = first
            && b.get(at..(at + length).min(b.len()))
                .is_some_and(|f| f.windows(4).any(|w| w == b"Xing" || w == b"Info"));
        if !tagged {
            samples += frame_samples as u64;
        }
        rate = frame_rate;
        first = false;
        at += length;
    }
    (samples > 0).then(|| samples as f32 / rate as f32 * 1000.0)
}

/// A layer III frame header's length in bytes, samples and sample rate.
fn header(h: &[u8]) -> Option<(usize, u32, u32)> {
    if h[0] != 0xff || h[1] & 0xe0 != 0xe0 {
        return None;
    }
    // 3 MPEG-1, 2 MPEG-2, 0 MPEG-2.5; layer III is 1.
    let version = (h[1] >> 3) & 3;
    if version == 1 || (h[1] >> 1) & 3 != 1 {
        return None;
    }
    let mpeg1 = version == 3;
    const V1: [u32; 15] = [
        0, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320,
    ];
    const V2: [u32; 15] = [0, 8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160];
    let bitrate = *if mpeg1 { &V1 } else { &V2 }.get((h[2] >> 4) as usize)? * 1000;
    let rate = *[44100, 48000, 32000].get(((h[2] >> 2) & 3) as usize)?
        / match version {
            3 => 1,
            2 => 2,
            _ => 4,
        };
    if bitrate == 0 {
        return None;
    }
    let padding = ((h[2] >> 1) & 1) as usize;
    let (samples, factor) = if mpeg1 { (1152, 144) } else { (576, 72) };
    let length = (factor * bitrate / rate) as usize + padding;
    Some((length, samples, rate))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wav(rate: u32, channels: u16, bits: u16, data: usize) -> Vec<u8> {
        let align = channels * bits / 8;
        let mut b = b"RIFF\0\0\0\0WAVEfmt ".to_vec();
        b.extend(16u32.to_le_bytes());
        b.extend(1u16.to_le_bytes());
        b.extend(channels.to_le_bytes());
        b.extend(rate.to_le_bytes());
        b.extend((rate * align as u32).to_le_bytes());
        b.extend(align.to_le_bytes());
        b.extend(bits.to_le_bytes());
        b.extend(b"data");
        b.extend((data as u32).to_le_bytes());
        b.extend(vec![0; data]);
        b
    }

    #[test]
    fn a_wav_plays_its_samples_over_its_rate() {
        // 22050 Hz, mono, 16 bits: 2 bytes a sample; 11025 samples are half a second.
        assert_eq!(length_ms(&wav(22050, 1, 16, 22050)), Some(500.0));
        assert_eq!(length_ms(&wav(22050, 2, 16, 0)), None);
    }

    /// A frame of MPEG-1 layer III at 128 kbit/s and 44100 Hz: 417 bytes (418
    /// padded), 1152 samples.
    fn frame(padded: bool, fill: u8) -> Vec<u8> {
        let mut f = vec![0xff, 0xfb, 0x90 | if padded { 2 } else { 0 }, 0x64];
        f.resize(if padded { 418 } else { 417 }, fill);
        f
    }

    #[test]
    fn an_mp3_plays_its_frames_samples() {
        let mut b = b"ID3\x04\0\0\0\0\0\x05".to_vec();
        b.extend([0; 5]);
        for i in 0..10 {
            b.extend(frame(i % 3 == 0, 0));
        }
        b.extend(b"TAG");
        let ms = length_ms(&b).unwrap();
        assert!((ms - 10.0 * 1152.0 / 44.1).abs() < 1e-3, "{ms}");
        // A first frame with a Xing header carries no sound.
        let mut tagged = frame(false, 0);
        tagged[36..40].copy_from_slice(b"Xing");
        tagged.extend(frame(false, 0));
        let ms = length_ms(&tagged).unwrap();
        assert!((ms - 1152.0 / 44.1).abs() < 1e-3, "{ms}");
        assert_eq!(length_ms(b"not a sound"), None);
    }

    #[test]
    fn a_sound_is_looked_for_as_a_wav_then_an_mp3() {
        let [wav, mp3] = files("sound/chars/luke/09luk001.mp3");
        assert_eq!(wav, "sound/chars/luke/09luk001.wav");
        assert_eq!(mp3, "sound/chars/luke/09luk001.mp3");
        assert_eq!(files("sound/a.b/c")[0], "sound/a.b/c.wav");
    }
}
