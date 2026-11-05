//! Independent IMA ADPCM blocks, 16 kHz mono, 40 ms. Dropped packets do not
//! desynchronise the decoder. PCM and media packets are never persisted.
use crate::{CoreError, Result};
use chacha20poly1305::{
    aead::{Aead, KeyInit, Payload},
    XChaCha20Poly1305, XNonce,
};
use hkdf::Hkdf;
use sha2::Sha256;
use zeroize::Zeroizing;
pub const SAMPLES: usize = 640;
const STEP: [i32; 89] = [
    7, 8, 9, 10, 11, 12, 13, 14, 16, 17, 19, 21, 23, 25, 28, 31, 34, 37, 41, 45, 50, 55, 60, 66,
    73, 80, 88, 97, 107, 118, 130, 143, 157, 173, 190, 209, 230, 253, 279, 307, 337, 371, 408, 449,
    494, 544, 598, 658, 724, 796, 876, 963, 1060, 1166, 1282, 1411, 1552, 1707, 1878, 2066, 2272,
    2499, 2749, 3024, 3327, 3660, 4026, 4428, 4871, 5358, 5894, 6484, 7132, 7845, 8630, 9493,
    10442, 11487, 12635, 13899, 15289, 16818, 18500, 20350, 22385, 24623, 27086, 29794, 32767,
];
const INDEX: [i32; 8] = [-1, -1, -1, -1, 2, 4, 6, 8];
fn bad() -> CoreError {
    CoreError::BadFrame
}
fn apply(code: u8, predictor: &mut i32, index: &mut i32) {
    let step = STEP[*index as usize];
    let mut diff = step / 8;
    if code & 1 != 0 {
        diff += step / 4;
    }
    if code & 2 != 0 {
        diff += step / 2;
    }
    if code & 4 != 0 {
        diff += step;
    }
    *predictor = (*predictor + if code & 8 != 0 { -diff } else { diff }).clamp(-32768, 32767);
    *index = (*index + INDEX[(code & 7) as usize]).clamp(0, 88);
}
pub fn encode(pcm: &[i16]) -> Result<Zeroizing<Vec<u8>>> {
    if pcm.len() != SAMPLES {
        return Err(bad());
    }
    let mut predictor = pcm[0] as i32;
    let mut index = 40;
    let mut out = Zeroizing::new(vec![0; 5 + (pcm.len() - 1).div_ceil(2)]);
    out[..2].copy_from_slice(&(pcm.len() as u16).to_be_bytes());
    out[2..4].copy_from_slice(&pcm[0].to_be_bytes());
    out[4] = index as u8;
    for (i, sample) in pcm[1..].iter().enumerate() {
        let mut delta = *sample as i32 - predictor;
        let mut code = if delta < 0 {
            delta = -delta;
            8
        } else {
            0
        };
        let mut step = STEP[index as usize];
        for bit in [4, 2, 1] {
            if delta >= step {
                code |= bit;
                delta -= step;
            }
            step /= 2;
        }
        apply(code, &mut predictor, &mut index);
        out[5 + i / 2] |= code << ((i % 2) * 4);
    }
    Ok(out)
}
pub fn decode(bytes: &[u8]) -> Result<Zeroizing<Vec<i16>>> {
    if bytes.len() != 5 + (SAMPLES - 1).div_ceil(2)
        || u16::from_be_bytes(bytes[..2].try_into().unwrap()) as usize != SAMPLES
        || bytes[4] > 88
    {
        return Err(bad());
    }
    let mut predictor = i16::from_be_bytes(bytes[2..4].try_into().unwrap()) as i32;
    let mut index = bytes[4] as i32;
    let mut out = Zeroizing::new(Vec::with_capacity(SAMPLES));
    out.push(predictor as i16);
    for i in 0..SAMPLES - 1 {
        let code = (bytes[5 + i / 2] >> ((i % 2) * 4)) & 15;
        apply(code, &mut predictor, &mut index);
        out.push(predictor as i16);
    }
    Ok(out)
}
/// Direction-specific keys, authenticated call ID, role, sequence and random
/// 192-bit nonce. Fresh random nonces also protect against accidental restarts.
pub struct Cipher {
    key: XChaCha20Poly1305,
    id: String,
    role: u8,
    sequence: u64,
    last: Option<u64>,
}
impl Cipher {
    pub fn new(secret: &str, id: &str, role: u8) -> Result<Self> {
        let secret = Zeroizing::new(hex::decode(secret).map_err(|_| bad())?);
        if secret.len() != 32 || role > 1 {
            return Err(bad());
        }
        let mut key = Zeroizing::new([0u8; 32]);
        let info = format!("valanium-call-audio-v1:{id}:{role}");
        Hkdf::<Sha256>::new(Some(b"valanium-call-v1"), &secret)
            .expand(info.as_bytes(), key.as_mut())
            .map_err(|_| bad())?;
        Ok(Self {
            key: XChaCha20Poly1305::new_from_slice(&*key).map_err(|_| bad())?,
            id: id.into(),
            role,
            sequence: 0,
            last: None,
        })
    }
    pub fn seal(&mut self, kind: u8, body: &[u8]) -> Result<Vec<u8>> {
        if body.len() > 1400 || kind > 2 {
            return Err(bad());
        }
        let seq = self.sequence;
        self.sequence = self.sequence.checked_add(1).ok_or_else(bad)?;
        let mut header = vec![1, self.role];
        header.extend_from_slice(&seq.to_be_bytes());
        let mut aad = self.id.as_bytes().to_vec();
        aad.extend_from_slice(&header);
        let nonce = crate::crypto::random_bytes(24);
        let mut plain = Zeroizing::new(vec![kind]);
        plain.extend_from_slice(body);
        let encrypted = self
            .key
            .encrypt(
                XNonce::from_slice(&nonce),
                Payload {
                    msg: &plain,
                    aad: &aad,
                },
            )
            .map_err(|_| bad())?;
        header.extend(nonce);
        header.extend(encrypted);
        Ok(header)
    }
    pub fn open(&mut self, wire: &[u8]) -> Result<(u8, Zeroizing<Vec<u8>>)> {
        if wire.len() < 51 || wire.len() > 1500 || wire[0] != 1 || wire[1] != self.role {
            return Err(bad());
        }
        let seq = u64::from_be_bytes(wire[2..10].try_into().unwrap());
        if self.last.is_some_and(|last| seq <= last) {
            return Err(bad());
        }
        let mut aad = self.id.as_bytes().to_vec();
        aad.extend_from_slice(&wire[..10]);
        let mut plain = Zeroizing::new(
            self.key
                .decrypt(
                    XNonce::from_slice(&wire[10..34]),
                    Payload {
                        msg: &wire[34..],
                        aad: &aad,
                    },
                )
                .map_err(|_| bad())?,
        );
        if plain.is_empty() || plain[0] > 2 {
            return Err(bad());
        }
        let kind = plain.remove(0);
        self.last = Some(seq);
        Ok((kind, plain))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn codec_preserves_voice_and_blocks_are_independent() {
        let pcm: Vec<i16> = (0..SAMPLES)
            .map(|i| ((i as f64 * std::f64::consts::TAU * 440. / 16000.).sin() * 10000.) as i16)
            .collect();
        let block = encode(&pcm).unwrap();
        let decoded = decode(&block).unwrap();
        let signal: f64 = pcm.iter().map(|x| (*x as f64).powi(2)).sum();
        let noise: f64 = pcm
            .iter()
            .zip(decoded.iter())
            .map(|(a, b)| (*a as f64 - *b as f64).powi(2))
            .sum();
        assert!(10. * (signal / noise).log10() > 20.);
        assert_eq!(decode(&block).unwrap().as_slice(), decoded.as_slice());
        assert!(decode(&block[..10]).is_err());
        assert!(encode(&pcm[..100]).is_err());
    }
    #[test]
    fn encryption_binds_direction_call_and_replay_and_tamper() {
        let key = "ab".repeat(32);
        let mut tx = Cipher::new(&key, "call", 0).unwrap();
        let mut rx = Cipher::new(&key, "call", 0).unwrap();
        let wire = tx.seal(1, b"audio").unwrap();
        let mut corrupt = wire.clone();
        *corrupt.last_mut().unwrap() ^= 1;
        assert!(rx.open(&corrupt).is_err());
        assert_eq!(&*rx.open(&wire).unwrap().1, b"audio");
        assert!(rx.open(&wire).is_err());
        assert!(Cipher::new(&key, "other", 0).unwrap().open(&wire).is_err());
        assert!(Cipher::new(&key, "call", 1).unwrap().open(&wire).is_err());
    }
}
