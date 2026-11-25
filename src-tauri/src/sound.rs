//! The reminder chime: an original four-note bell figure synthesised at
//! run time (no audio files), played with the Windows `PlaySound` API.

use std::sync::Mutex;

const RATE: u32 = 22_050;

/// Kept alive while it plays (`SND_ASYNC` reads from this buffer).
static BUF: Mutex<Option<Vec<u8>>> = Mutex::new(None);

/// Rising "departure" figure in E major, then a soft resolution.
const NOTES: [(f64, f64, f64); 5] = [
    // (frequency Hz, start s, length s)
    (659.25, 0.00, 0.55), // E5
    (830.61, 0.16, 0.55), // G#5
    (987.77, 0.32, 0.60), // B5
    (1318.5, 0.48, 1.10), // E6
    (987.77, 0.80, 0.90), // B5 (echo)
];

pub fn wav(volume: f64) -> Vec<u8> {
    let vol = volume.clamp(0.0, 1.0);
    let total = 1.9_f64;
    let n = (total * RATE as f64) as usize;
    let mut s = vec![0f64; n];
    for (f, t0, len) in NOTES {
        let a = (t0 * RATE as f64) as usize;
        let m = (len * RATE as f64) as usize;
        for i in 0..m {
            let idx = a + i;
            if idx >= n {
                break;
            }
            let t = i as f64 / RATE as f64;
            let attack = (t / 0.006).min(1.0);
            let env = attack * (-t * 4.2).exp();
            let x = (2.0 * std::f64::consts::PI * f * t).sin()
                + 0.35 * (2.0 * std::f64::consts::PI * f * 2.0 * t).sin() * (-t * 7.0).exp()
                + 0.12 * (2.0 * std::f64::consts::PI * f * 3.01 * t).sin() * (-t * 11.0).exp();
            s[idx] += x * env * 0.24;
        }
    }
    let data: Vec<i16> = s.iter().map(|x| ((x * vol).clamp(-1.0, 1.0) * 32_000.0) as i16).collect();
    let bytes = (data.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + bytes as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + bytes).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&RATE.to_le_bytes());
    out.extend_from_slice(&(RATE * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&bytes.to_le_bytes());
    for v in data {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out
}

#[cfg(windows)]
pub fn chime(volume: f64) {
    use windows_sys::Win32::Media::Audio::{PlaySoundW, SND_ASYNC, SND_MEMORY, SND_NODEFAULT};
    let mut guard = BUF.lock().unwrap_or_else(|p| p.into_inner());
    unsafe {
        // Stop whatever is playing before replacing the buffer it reads.
        PlaySoundW(std::ptr::null(), std::ptr::null_mut(), 0);
    }
    *guard = Some(wav(volume));
    if let Some(b) = guard.as_ref() {
        unsafe {
            PlaySoundW(b.as_ptr() as *const u16, std::ptr::null_mut(), SND_MEMORY | SND_ASYNC | SND_NODEFAULT);
        }
    }
}

#[cfg(not(windows))]
pub fn chime(_volume: f64) {
    let _ = &BUF;
}

#[cfg(test)]
mod tests {
    #[test]
    fn wav_header_is_valid() {
        let w = super::wav(0.5);
        assert_eq!(&w[0..4], b"RIFF");
        assert_eq!(&w[8..12], b"WAVE");
        let len = u32::from_le_bytes([w[40], w[41], w[42], w[43]]) as usize;
        assert_eq!(len + 44, w.len());
    }
}
