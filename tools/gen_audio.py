"""Synthesize the original MGS-inspired sound set for the simulator.

Run from the repo root:  python3 tools/gen_audio.py

All sounds are original synthesis (no sampled/ripped assets): dual-tone
codec-flavored beeps, a minor-second alert stab, and a band-limited sawtooth
stepper hum authored as a seamless loop (pitch-shifted at runtime by head
speed). Uses numpy when available, pure stdlib otherwise.

Output: app/assets/audio/*.wav (mono, 22050 Hz, 16-bit).
"""

import math
import os
import struct
import wave

RATE = 22050
OUT_DIR = os.path.join("app", "assets", "audio")

try:
    import numpy as np
except ImportError:
    np = None


def write_wav(name, samples):
    """samples: list/array of floats in [-1, 1]."""
    os.makedirs(OUT_DIR, exist_ok=True)
    path = os.path.join(OUT_DIR, name)
    with wave.open(path, "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(RATE)
        frames = bytearray()
        for s in samples:
            frames += struct.pack("<h", int(max(-1.0, min(1.0, s)) * 32767))
        w.writeframes(bytes(frames))
    print(f"wrote {path} ({len(samples) / RATE:.2f}s)")


def env_ad(n, attack_s, decay_s):
    """Attack/decay envelope over n samples."""
    a = max(1, int(attack_s * RATE))
    out = []
    for i in range(n):
        if i < a:
            out.append(i / a)
        else:
            out.append(math.exp(-(i - a) / (decay_s * RATE)))
    return out


def sine(freq, n, phase=0.0):
    return [math.sin(2 * math.pi * freq * i / RATE + phase) for i in range(n)]


def square(freq, n):
    return [1.0 if math.sin(2 * math.pi * freq * i / RATE) >= 0 else -1.0 for i in range(n)]


def band_limited_saw(freq, n, harmonics=None):
    """Additive saw: no aliasing, and exact periodicity for clean loops."""
    if harmonics is None:
        harmonics = int((RATE / 2) / freq) - 1
    out = [0.0] * n
    for k in range(1, harmonics + 1):
        amp = 1.0 / k
        for i in range(n):
            out[i] += amp * math.sin(2 * math.pi * freq * k * i / RATE)
    peak = max(abs(s) for s in out)
    return [s / peak for s in out]


def mix(*tracks):
    n = max(len(t) for t in tracks)
    out = [0.0] * n
    for t in tracks:
        for i, s in enumerate(t):
            out[i] += s
    return out


def gain(track, g):
    return [s * g for s in track]


def codec_burst(f1, f2, dur_s):
    n = int(dur_s * RATE)
    env = env_ad(n, 0.004, dur_s * 0.9)
    tone = mix(gain(sine(f1, n), 0.5), gain(sine(f2, n), 0.5))
    return [t * e for t, e in zip(tone, env)]


def silence(dur_s):
    return [0.0] * int(dur_s * RATE)


def codec_call():
    """Two dual-sine ring bursts, codec-flavored (original frequencies)."""
    burst = codec_burst(1329.0, 1992.0, 0.09)
    seq = burst + silence(0.06) + burst + silence(0.10)
    return gain(seq + seq, 0.7)


def codec_beep():
    return gain(codec_burst(1245.0, 1867.0, 0.08), 0.6)


def ui_click():
    n = int(0.015 * RATE)
    env = env_ad(n, 0.001, 0.004)
    tone = square(2200.0, n)
    return [t * e * 0.35 for t, e in zip(tone, env)]


def alert():
    """The '!' sting: square+saw minor-second stab (E5 -> F5) + noise burst."""
    n1 = int(0.16 * RATE)
    n2 = int(0.34 * RATE)
    e5, f5 = 659.3, 698.5

    stab1 = mix(gain(square(e5, n1), 0.4), gain(band_limited_saw(e5, n1), 0.5))
    stab2 = mix(gain(square(f5, n2), 0.4), gain(band_limited_saw(f5, n2), 0.5))

    seed = 0x2545F491
    noise = []
    for i in range(int(0.05 * RATE)):
        seed = (seed * 1103515245 + 12345) & 0x7FFFFFFF
        noise.append((seed / 0x3FFFFFFF - 1.0) * math.exp(-i / (0.012 * RATE)))

    env1 = env_ad(n1, 0.002, 0.20)
    env2 = env_ad(n2, 0.002, 0.16)
    out = mix(
        [s * e for s, e in zip(stab1, env1)],
        silence(0.16) + [s * e for s, e in zip(stab2, env2)],
        gain(noise, 0.5),
    )
    return gain(out, 0.55)


def stepper_hum():
    """Seamless 2.0s loop: 110 Hz band-limited saw with 5 Hz vibrato.

    Loop-safe by construction: 110 Hz x 2.0s = 220 carrier cycles and
    5 Hz x 2.0s = 10 vibrato cycles, both integers, and the vibrato's phase
    contribution integrates to zero over whole vibrato cycles - so the phase
    at the loop point wraps exactly to its starting value.
    """
    dur = 2.0
    n = int(dur * RATE)
    base = 110.0
    vib_hz = 5.0
    vib_depth = 1.5

    # Phase-integrated FM so vibrato keeps the loop click-free.
    out = []
    phase = 0.0
    for i in range(n):
        freq = base + vib_depth * math.sin(2 * math.pi * vib_hz * i / RATE)
        phase += 2 * math.pi * freq / RATE
        # 6 harmonics of a saw, band-limited by construction
        s = sum(math.sin(phase * k) / k for k in range(1, 7))
        out.append(s)
    peak = max(abs(s) for s in out)
    return [0.5 * s / peak for s in out]


def main():
    write_wav("codec_call.wav", codec_call())
    write_wav("codec_beep.wav", codec_beep())
    write_wav("ui_click.wav", ui_click())
    write_wav("alert.wav", alert())
    write_wav("stepper_hum.wav", stepper_hum())


if __name__ == "__main__":
    main()
