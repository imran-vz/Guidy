#!/usr/bin/env python3
"""Generate the "mechanical" feedback sounds. Standard library only.

    python3 -I scripts/gen_key_sounds.py           write the WAVs
    python3 -I scripts/gen_key_sounds.py --check   verify the committed WAVs

Writes assets/resources/mechanical_{start,stop,error}.wav: 48 kHz, mono, 16-bit,
with the first transient on sample 0, a 3 ms fade at the end of the audible part,
then 18 ms of digital zeros. Everything is synthesized here from seeded noise and
damped sines, so no third-party audio is involved.

Components:
  tone(f, tau, a)         a * sin(2*pi*f*t) * exp(-t/tau)
  noise(hp, lp, tau, a)   seeded noise through one-pole high-pass and low-pass
                          filters, scaled to unit peak, then a * exp(-t/tau)
Frequencies are in Hz and tau in ms. A high-pass of 0 means no high-pass filter.
Each component's t starts at its layer's offset. Each noise component takes the
next run of values from the sound's generator, in the order listed below.
"""

import math
import os
import struct
import sys
import wave

RATE = 48000
FADE_MS = 3
SILENCE_MS = 18
MAX_TOTAL_MS = 80
FULL_SCALE = 32767
LSB_TOLERANCE = 1
HEADER_SIZE = 44

RES = os.path.join(
    os.path.dirname(os.path.dirname(os.path.abspath(__file__))), "assets", "resources"
)


def tone(freq, tau, amp):
    return ("tone", freq, tau, amp)


def noise(hp, lp, tau, amp):
    return ("noise", hp, lp, tau, amp)


def error_hit(low_freq, gain):
    return [
        tone(low_freq, 12, 1.0 * gain),
        tone(320, 7, 0.4 * gain),
        tone(780, 5, 0.5 * gain),
        noise(0, 1400, 2, 0.5 * gain),
    ]


# name: (seed, audible ms, peak, [(offset ms, components), ...])
SOUNDS = {
    "start": (
        0x4B01,
        62,
        0.50,
        [
            (
                0,
                [
                    noise(2500, 9000, 1.0, 0.55),
                    tone(3100, 1.2, 0.3),
                    tone(5200, 9, 0.035),
                    tone(7300, 6, 0.02),
                ],
            ),
            (
                7,
                [
                    tone(210, 14, 1.0),
                    tone(430, 8, 0.45),
                    tone(1150, 6, 0.3),
                    noise(400, 2500, 3, 0.45),
                ],
            ),
        ],
    ),
    "stop": (
        0x4B02,
        46,
        0.40,
        [
            (
                0,
                [
                    noise(3500, 11000, 0.7, 0.35),
                    tone(6100, 8, 0.03),
                ],
            ),
            (
                4,
                [
                    tone(2400, 3.5, 0.7),
                    tone(1450, 5, 0.5),
                    tone(340, 9, 0.4),
                    noise(1500, 8000, 1.5, 0.4),
                ],
            ),
        ],
    ),
    "error": (
        0x4B03,
        62,
        0.50,
        [
            (0, error_hit(150, 1.0)),
            (30, error_hit(125, 0.8)),
        ],
    ),
}


class Lcg:
    """32-bit linear congruential generator (Numerical Recipes constants)."""

    def __init__(self, seed):
        self.state = seed & 0xFFFFFFFF

    def next(self):
        self.state = (1664525 * self.state + 1013904223) & 0xFFFFFFFF
        return self.state / 2147483648.0 - 1.0


def ms_to_samples(ms):
    return int(round(ms * RATE / 1000))


def filtered_noise(rng, hp, lp, count):
    """One-pole high-pass then low-pass over `count` noise values, unit peak."""
    hp_a = 1.0 / (1.0 + 2.0 * math.pi * hp / RATE) if hp > 0 else None
    lp_a = 1.0 - math.exp(-2.0 * math.pi * lp / RATE)
    prev_in = 0.0
    prev_hp = 0.0
    low = 0.0
    out = []
    for _ in range(count):
        value = rng.next()
        if hp_a is not None:
            prev_hp = hp_a * (prev_hp + value - prev_in)
            prev_in = value
            value = prev_hp
        low += lp_a * (value - low)
        out.append(low)
    peak = max(abs(v) for v in out)
    return [v / peak for v in out]


def add_component(buf, rng, offset, component):
    count = len(buf) - offset
    if component[0] == "tone":
        _, freq, tau, amp = component
        for i in range(count):
            t = i / RATE
            buf[offset + i] += (
                amp * math.sin(2.0 * math.pi * freq * t) * math.exp(-t * 1000.0 / tau)
            )
    else:
        _, hp, lp, tau, amp = component
        values = filtered_noise(rng, hp, lp, count)
        for i, value in enumerate(values):
            t = i / RATE
            buf[offset + i] += amp * value * math.exp(-t * 1000.0 / tau)


def synthesize(name):
    """Return the sound as a list of 16-bit integers."""
    seed, audible_ms, target_peak, layers = SOUNDS[name]
    audible = ms_to_samples(audible_ms)
    buf = [0.0] * audible
    rng = Lcg(seed)
    for offset_ms, components in layers:
        for component in components:
            add_component(buf, rng, ms_to_samples(offset_ms), component)

    fade = ms_to_samples(FADE_MS)
    for i in range(fade):
        # Raised cosine. The half-sample offset keeps the ramp strictly inside 0..1.
        gain = math.cos(0.5 * math.pi * (i + 0.5) / fade) ** 2
        buf[audible - fade + i] *= gain

    scale = target_peak / max(abs(v) for v in buf)
    samples = [int(round(v * scale * FULL_SCALE)) for v in buf]
    return samples + [0] * ms_to_samples(SILENCE_MS)


def wav_path(name):
    return os.path.join(RES, "mechanical_{}.wav".format(name))


def canonical_header(data_size):
    """The header `write_wav` produces for mono 16-bit PCM at RATE."""
    return (
        b"RIFF"
        + struct.pack("<I", 36 + data_size)
        + b"WAVEfmt "
        + struct.pack("<IHHIIHH", 16, 1, 1, RATE, RATE * 2, 2, 16)
        + b"data"
        + struct.pack("<I", data_size)
    )


def write_wav(path, samples):
    with wave.open(path, "wb") as wav:
        wav.setnchannels(1)
        wav.setsampwidth(2)
        wav.setframerate(RATE)
        wav.writeframes(b"".join(s.to_bytes(2, "little", signed=True) for s in samples))


def read_wav(path):
    """Parse a canonical 44-byte PCM WAV, rejecting anything else.

    `wave` tolerates bad block alignment, byte rates, encoding tags and truncated
    payloads, so the header is compared field by field against the raw bytes.
    """
    with open(path, "rb") as handle:
        raw = handle.read()
    if len(raw) < HEADER_SIZE:
        raise ValueError("shorter than a {}-byte WAV header".format(HEADER_SIZE))
    data_size = len(raw) - HEADER_SIZE
    expected = canonical_header(data_size)
    if raw[:HEADER_SIZE] != expected:
        raise ValueError(
            "header is {} but expected {}".format(
                raw[:HEADER_SIZE].hex(), expected.hex()
            )
        )
    if data_size % 2:
        raise ValueError("payload has a partial 16-bit sample")
    return [
        int.from_bytes(raw[i : i + 2], "little", signed=True)
        for i in range(HEADER_SIZE, len(raw), 2)
    ]


def check_sound(name):
    """Return a list of problems with the committed WAV for `name`."""
    _, audible_ms, target_peak, _ = SOUNDS[name]
    path = wav_path(name)
    if not os.path.exists(path):
        return ["missing file {}".format(path)]
    try:
        committed = read_wav(path)
    except (OSError, ValueError) as err:
        return ["invalid WAV: {}".format(err)]

    problems = []
    audible = ms_to_samples(audible_ms)
    expected_len = audible + ms_to_samples(SILENCE_MS)
    if len(committed) != expected_len:
        problems.append("expected {} frames, got {}".format(expected_len, len(committed)))
    if len(committed) > ms_to_samples(MAX_TOTAL_MS):
        problems.append("longer than {} ms".format(MAX_TOTAL_MS))

    fresh = synthesize(name)
    if len(fresh) == len(committed):
        worst = max(abs(a - b) for a, b in zip(fresh, committed))
        if worst > LSB_TOLERANCE:
            problems.append(
                "differs from regenerated audio by {} LSB (limit {})".format(
                    worst, LSB_TOLERANCE
                )
            )

    if not committed:
        problems.append("no audio frames")
        return problems

    peak = max(abs(s) for s in committed)
    expected_peak = target_peak * FULL_SCALE
    if abs(peak - expected_peak) > LSB_TOLERANCE + 0.5:
        problems.append("peak {} but expected {:.1f}".format(peak, expected_peak))
    if committed[0] == 0:
        problems.append("sample 0 is silent")

    silence = ms_to_samples(SILENCE_MS)
    if any(committed[-silence:]):
        problems.append("last {} ms are not digital zeros".format(SILENCE_MS))
    return problems


def run_check():
    failed = False
    for name in SOUNDS:
        problems = check_sound(name)
        for problem in problems:
            failed = True
            print("FAIL {}: {}".format(name, problem))
        if not problems:
            print("ok   {}".format(name))
    return 1 if failed else 0


def run_generate():
    os.makedirs(RES, exist_ok=True)
    for name in SOUNDS:
        path = wav_path(name)
        samples = synthesize(name)
        write_wav(path, samples)
        print("wrote {} ({} frames)".format(path, len(samples)))
    return 0


def main(argv):
    if argv == ["--check"]:
        return run_check()
    if not argv:
        return run_generate()
    print(__doc__, file=sys.stderr)
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
