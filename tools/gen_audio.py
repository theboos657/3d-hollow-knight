#!/usr/bin/env python3
"""Procedural audio: synthesises every sound effect and music loop as WAV.

Stdlib only (math, random, struct, wave). Deterministic: a fixed seed per
sound, so re-running only changes files whose recipe changed.

    python3 tools/gen_audio.py            # write assets/audio/{sfx,music}/*.wav
    python3 tools/gen_audio.py --check    # write nothing; analyse what would be written

The sounds are deliberately simple (a bell-and-drone palette to match the
game's tone). Recipes live in SFX and MUSIC at the bottom of this file.
"""

import math
import os
import random
import struct
import sys
import wave

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")
OUT = os.path.join(ROOT, "assets", "audio")
SR = 22050  # sound effects
MSR = 16000  # music (mostly low, soft content)
TAU = math.pi * 2


# ------------------------------------------------------------------ toolkit --


def n_samples(seconds, sr=SR):
    return max(1, int(seconds * sr))


def env_adsr(n, a=0.005, d=0.05, s=0.6, r=0.05, sr=SR):
    """Attack / decay / sustain-level / release envelope over n samples."""
    a_n, d_n, r_n = int(a * sr), int(d * sr), int(r * sr)
    out = []
    for i in range(n):
        if i < a_n:
            v = i / max(1, a_n)
        elif i < a_n + d_n:
            v = 1.0 - (1.0 - s) * (i - a_n) / max(1, d_n)
        elif i > n - r_n:
            v = s * (n - i) / max(1, r_n)
        else:
            v = s
        out.append(max(0.0, v))
    return out


def env_decay(n, tau, sr=SR):
    """Exponential decay with time constant tau (seconds)."""
    return [math.exp(-i / (tau * sr)) for i in range(n)]


def sine(freq, n, sr=SR, phase=0.0):
    return [math.sin(TAU * freq * i / sr + phase) for i in range(n)]


def sweep(f0, f1, n, sr=SR, curve=1.0):
    """Sine gliding from f0 to f1 (curve > 1 lingers near f0)."""
    out, ph = [], 0.0
    for i in range(n):
        t = (i / max(1, n - 1)) ** curve
        f = f0 + (f1 - f0) * t
        ph += TAU * f / sr
        out.append(math.sin(ph))
    return out


def saw_sweep(f0, f1, n, sr=SR):
    out, ph = [], 0.0
    for i in range(n):
        f = f0 + (f1 - f0) * i / max(1, n - 1)
        ph += f / sr
        out.append(2.0 * (ph % 1.0) - 1.0)
    return out


def noise(n, rng):
    return [rng.uniform(-1.0, 1.0) for _ in range(n)]


def lowpass(x, cutoff, sr=SR):
    a = 1.0 - math.exp(-TAU * cutoff / sr)
    y, out = 0.0, []
    for v in x:
        y += a * (v - y)
        out.append(y)
    return out


def highpass(x, cutoff, sr=SR):
    lp = lowpass(x, cutoff, sr)
    return [v - l for v, l in zip(x, lp)]


def bandpass_sweep(x, f0, f1, q_width=0.6, sr=SR):
    """Crude sweeping band: a lowpass at the moving centre minus one below it."""
    n = len(x)
    y1 = y2 = 0.0
    out = []
    for i, v in enumerate(x):
        f = f0 + (f1 - f0) * i / max(1, n - 1)
        a1 = 1.0 - math.exp(-TAU * f / sr)
        a2 = 1.0 - math.exp(-TAU * f * q_width / sr)
        y1 += a1 * (v - y1)
        y2 += a2 * (v - y2)
        out.append(y1 - y2)
    return out


def mul(a, b):
    return [x * y for x, y in zip(a, b)]


def scale(a, k):
    return [x * k for x in a]


def add(*tracks):
    n = max(len(t) for t in tracks)
    out = [0.0] * n
    for t in tracks:
        for i, v in enumerate(t):
            out[i] += v
    return out


def delay(x, seconds, sr=SR):
    return [0.0] * int(seconds * sr) + list(x)


def bell(freq, seconds, sr=SR, brightness=1.0):
    """A struck bell: inharmonic partials, each with its own decay."""
    n = n_samples(seconds, sr)
    partials = [
        (0.5, 1.0, 1.00),
        (1.0, 1.0, 0.85),
        (1.19, 0.6, 0.55),
        (1.56, 0.5, 0.45),
        (2.0, 0.55, 0.35),
        (2.74, 0.35 * brightness, 0.22),
        (3.76, 0.25 * brightness, 0.14),
    ]
    out = [0.0] * n
    for ratio, amp, decay_frac in partials:
        f = freq * ratio
        if f > sr * 0.45:
            continue
        env = env_decay(n, seconds * decay_frac * 0.55, sr)
        for i in range(n):
            out[i] += amp * env[i] * math.sin(TAU * f * i / sr)
    # a tiny strike transient
    for i in range(min(n, int(0.004 * sr))):
        out[i] += 0.6 * (1.0 - i / (0.004 * sr)) * math.sin(TAU * freq * 5.0 * i / sr)
    return out


def fade_edges(x, ms=4, sr=SR):
    n = int(ms * 0.001 * sr)
    x = list(x)
    for i in range(min(n, len(x) // 2)):
        k = i / n
        x[i] *= k
        x[-1 - i] *= k
    return x


def normalise(x, peak=0.85):
    m = max((abs(v) for v in x), default=0.0)
    if m < 1e-9:
        return list(x)
    return [v * peak / m for v in x]


def soft_clip(x, drive=1.0):
    return [math.tanh(v * drive) for v in x]


def fold_loop(x, n):
    """Make a loop of exactly n samples: the tail beyond n wraps onto the start."""
    out = list(x[:n]) + [0.0] * max(0, n - len(x))
    for i in range(n, len(x)):
        out[i - n] += x[i]
    return out


# --------------------------------------------------------------------- SFX --


def sfx_jump(rng):
    n = n_samples(0.11)
    return mul(sweep(280, 620, n), env_adsr(n, 0.003, 0.03, 0.4, 0.05))


def sfx_land(rng):
    n = n_samples(0.12)
    thud = mul(sweep(110, 45, n), env_decay(n, 0.04))
    dust = mul(lowpass(noise(n, rng), 900), env_decay(n, 0.03))
    return add(scale(thud, 1.0), scale(dust, 0.7))


def sfx_dash(rng):
    n = n_samples(0.2)
    w = bandpass_sweep(noise(n, rng), 600, 3200)
    return mul(scale(w, 5.0), env_adsr(n, 0.01, 0.06, 0.6, 0.1))


def sfx_swing(rng):
    n = n_samples(0.1)
    w = bandpass_sweep(noise(n, rng), 1200, 4200)
    return mul(scale(w, 4.0), env_adsr(n, 0.004, 0.03, 0.5, 0.05))


def sfx_hit(rng):
    n = n_samples(0.16)
    click = mul(highpass(noise(n, rng), 1500), env_decay(n, 0.012))
    thump = mul(sweep(220, 90, n), env_decay(n, 0.05))
    return add(scale(click, 1.2), scale(thump, 1.0))


def sfx_pogo(rng):
    n = n_samples(0.22)
    fm = [math.sin(TAU * 24 * i / SR) * 0.06 for i in range(n)]
    base = sweep(180, 560, n, curve=0.6)
    tone = [math.sin(math.asin(max(-1, min(1, b))) + f) for b, f in zip(base, fm)]
    return mul(tone, env_adsr(n, 0.004, 0.06, 0.5, 0.1))


def sfx_hurt(rng):
    n = n_samples(0.3)
    body = mul(saw_sweep(330, 70, n), env_decay(n, 0.12))
    grit = mul(lowpass(noise(n, rng), 2500), env_decay(n, 0.05))
    return soft_clip(add(scale(body, 0.7), scale(grit, 0.6)), 1.4)


def sfx_block(rng):
    n = n_samples(0.35)
    parts = []
    for f, a, tau in [(820, 1.0, 0.09), (1330, 0.8, 0.07), (2110, 0.6, 0.05), (3050, 0.35, 0.03)]:
        parts.append(scale(mul(sine(f, n), env_decay(n, tau)), a))
    click = mul(highpass(noise(n, rng), 2500), env_decay(n, 0.006))
    return add(*parts, scale(click, 0.8))


def sfx_enemy_die(rng):
    n = n_samples(0.28)
    pop = mul(highpass(noise(n, rng), 800), env_decay(n, 0.02))
    fall = mul(sweep(420, 90, n, curve=0.7), env_decay(n, 0.08))
    return add(scale(pop, 0.9), scale(fall, 0.9))


def sfx_focus(rng):
    """A loopable shimmer (1 s): two detuned tones slowly rising and falling."""
    n = n_samples(1.0)
    a = [math.sin(TAU * 440 * i / SR + 2.0 * math.sin(TAU * 2 * i / SR)) for i in range(n)]
    b = [math.sin(TAU * 660 * i / SR + 1.5 * math.sin(TAU * 3 * i / SR)) for i in range(n)]
    trem = [0.6 + 0.4 * math.sin(TAU * 4 * i / SR) for i in range(n)]
    return mul(add(scale(a, 0.6), scale(b, 0.4)), trem)


def sfx_heal(rng):
    notes = [523.25, 659.25, 783.99, 1046.5]
    out = []
    for k, f in enumerate(notes):
        n = n_samples(0.5)
        t = mul(add(sine(f, n), scale(sine(f * 2, n), 0.3)), env_decay(n, 0.18))
        out = add(out, delay(t, 0.07 * k)) if out else t
    return out


def sfx_cast(rng):
    n = n_samples(0.2)
    z = mul(saw_sweep(900, 180, n), env_decay(n, 0.07))
    w = bandpass_sweep(noise(n, rng), 3000, 900)
    return add(scale(z, 0.6), scale(w, 3.0))


def sfx_telegraph(rng):
    out = []
    for k in range(2):
        n = n_samples(0.045)
        b = mul(sine(1250, n), env_adsr(n, 0.002, 0.015, 0.5, 0.02))
        out = add(out, delay(b, 0.07 * k)) if out else b
    return out


def sfx_boss_toll(rng):
    return bell(98.0, 2.2)


def sfx_boss_slam(rng):
    n = n_samples(0.7)
    boom = mul(sweep(70, 32, n, curve=0.5), env_decay(n, 0.22))
    rumble = mul(lowpass(noise(n, rng), 220), env_decay(n, 0.2))
    crack = mul(highpass(noise(n, rng), 1200), env_decay(n, 0.02))
    return soft_clip(add(scale(boom, 1.0), scale(rumble, 1.6), scale(crack, 0.5)), 1.3)


def sfx_boss_roar(rng):
    n = n_samples(1.6)
    core = saw_sweep(88, 55, n)
    core2 = saw_sweep(93, 58, n)
    vowel = bandpass_sweep(add(core, core2), 350, 900, 0.7)
    breath = lowpass(noise(n, rng), 600)
    env = env_adsr(n, 0.12, 0.3, 0.8, 0.5)
    return soft_clip(mul(add(scale(vowel, 2.5), scale(breath, 0.5)), env), 1.6)


def sfx_boss_die(rng):
    n = n_samples(3.0)
    rumble = mul(lowpass(noise(n, rng), 160), env_adsr(n, 0.05, 0.5, 0.6, 1.5))
    sink = mul(sweep(120, 30, n, curve=0.6), env_adsr(n, 0.02, 0.4, 0.7, 1.4))
    return add(scale(rumble, 2.0), scale(sink, 1.0), delay(scale(bell(82.0, 2.6), 0.9), 0.3))


def sfx_bell_fall(rng):
    n = n_samples(0.35)
    w = mul(sweep(1100, 380, n, curve=0.8), env_adsr(n, 0.02, 0.1, 0.5, 0.12))
    return scale(w, 0.7)


def sfx_bell_hit(rng):
    return scale(bell(330.0, 0.9), 0.9)


def sfx_bench(rng):
    out = []
    for k, f in enumerate([392.0, 494.0, 587.3]):
        out = add(out, delay(scale(bell(f, 1.6, brightness=0.5), 0.8), 0.12 * k)) if out else scale(bell(f, 1.6, brightness=0.5), 0.8)
    return out


def sfx_pickup(rng):
    out = []
    for k, f in enumerate([392.0, 523.3, 659.3, 784.0, 1046.5]):
        n = n_samples(0.9)
        t = mul(add(sine(f, n), scale(sine(f * 3, n), 0.2)), env_decay(n, 0.3))
        out = add(out, delay(t, 0.09 * k)) if out else t
    return out


def sfx_death(rng):
    return add(scale(bell(73.4, 2.2), 1.0), scale(sfx_hurt(rng), 0.5))


def sfx_ui_move(rng):
    n = n_samples(0.04)
    return mul(sine(900, n), env_adsr(n, 0.002, 0.01, 0.5, 0.015))


def sfx_ui_select(rng):
    n = n_samples(0.12)
    return add(mul(sine(660, n), env_decay(n, 0.05)), mul(sine(990, n), env_decay(n, 0.04)))


def sfx_wall_jump(rng):
    n = n_samples(0.12)
    return add(scale(mul(sweep(250, 500, n), env_adsr(n, 0.003, 0.03, 0.4, 0.05)), 0.8),
               scale(mul(bandpass_sweep(noise(n, rng), 1500, 3000), env_decay(n, 0.04)), 2.0))


def sfx_door(rng):
    n = n_samples(0.25)
    return mul(scale(bandpass_sweep(noise(n, rng), 300, 900), 4.0), env_adsr(n, 0.05, 0.05, 0.6, 0.12))


SFX = {
    "jump": (sfx_jump, 0.5),
    "wall_jump": (sfx_wall_jump, 0.5),
    "land": (sfx_land, 0.55),
    "dash": (sfx_dash, 0.6),
    "swing": (sfx_swing, 0.5),
    "hit": (sfx_hit, 0.75),
    "pogo": (sfx_pogo, 0.6),
    "hurt": (sfx_hurt, 0.85),
    "block": (sfx_block, 0.65),
    "enemy_die": (sfx_enemy_die, 0.7),
    "focus": (sfx_focus, 0.35),
    "heal": (sfx_heal, 0.6),
    "cast": (sfx_cast, 0.6),
    "telegraph": (sfx_telegraph, 0.55),
    "boss_toll": (sfx_boss_toll, 0.85),
    "boss_slam": (sfx_boss_slam, 0.9),
    "boss_roar": (sfx_boss_roar, 0.9),
    "boss_die": (sfx_boss_die, 0.9),
    "bell_fall": (sfx_bell_fall, 0.5),
    "bell_hit": (sfx_bell_hit, 0.7),
    "bench": (sfx_bench, 0.7),
    "pickup": (sfx_pickup, 0.7),
    "death": (sfx_death, 0.85),
    "ui_move": (sfx_ui_move, 0.4),
    "ui_select": (sfx_ui_select, 0.55),
    "door": (sfx_door, 0.4),
}


# ------------------------------------------------------------------- music --
# Loops. Frequencies are snapped to whole cycles per loop so the seam is
# silent; decaying notes wrap around the end (fold_loop).

NOTE = {
    "C2": 65.41, "D2": 73.42, "E2": 82.41, "F2": 87.31, "G2": 98.0, "A2": 110.0, "Bb2": 116.54,
    "C3": 130.81, "D3": 146.83, "E3": 164.81, "F3": 174.61, "G3": 196.0, "A3": 220.0, "Bb3": 233.08,
    "C4": 261.63, "D4": 293.66, "E4": 329.63, "F4": 349.23, "G4": 392.0, "A4": 440.0, "Bb4": 466.16,
    "C5": 523.25, "D5": 587.33,
}


def snap(f, seconds):
    """Nearest frequency with a whole number of cycles in `seconds`."""
    return max(1, round(f * seconds)) / seconds


def pad(freqs, seconds, sr, detune=0.004, slow=0.0):
    """Soft sustained chord: detuned pairs of sines, breathing amplitude."""
    n = n_samples(seconds, sr)
    out = [0.0] * n
    for f in freqs:
        for d in (1 - detune, 1 + detune):
            f2 = snap(f * d, seconds)
            for i in range(n):
                out[i] += math.sin(TAU * f2 * i / sr) / (2 * len(freqs))
    if slow:
        k = max(1, round(slow * seconds))
        for i in range(n):
            out[i] *= 0.75 + 0.25 * math.sin(TAU * k * i / n)
    return out


def pluck(f, seconds, sr, tau=0.5):
    n = n_samples(seconds, sr)
    env = env_decay(n, tau, sr)
    return [
        env[i] * (math.sin(TAU * f * i / sr) + 0.35 * math.sin(TAU * 2 * f * i / sr) + 0.15 * math.sin(TAU * 3 * f * i / sr))
        for i in range(n)
    ]


def place(track, event, at, sr, gain=1.0):
    start = int(at * sr)
    for i, v in enumerate(event):
        if start + i < len(track):
            track[start + i] += v * gain
    return track


def music_ambient(seed, root, chord, notes, seconds=12.0, pulse=None):
    rng = random.Random(seed)
    sr = MSR
    n = n_samples(seconds, sr)
    total = n + n_samples(4.0, sr)  # headroom so notes can ring past the end
    track = [0.0] * total
    low = pad([NOTE[root]], seconds, sr, slow=1)
    mid = pad([NOTE[c] for c in chord], seconds, sr, detune=0.006, slow=2)
    for i in range(n):
        track[i] += 0.55 * low[i] + 0.35 * mid[i]
    step = seconds / len(notes)
    for k, name in enumerate(notes):
        if name is None:
            continue
        at = k * step + rng.uniform(-0.05, 0.05) * step
        ev = bell(NOTE[name], 3.0, sr, brightness=0.5)
        place(track, ev, max(0.0, at), sr, 0.14)
    if pulse:
        beat = seconds / pulse
        for k in range(pulse):
            ev = pluck(NOTE[root] / 2, 0.6, sr, 0.18)
            place(track, ev, k * beat, sr, 0.5)
    return fold_loop(track, n)


def music_drive(seed, root, seconds=12.0, bpm=96):
    """Boss music: a low pulse, off-beat bell strikes, a tense pad."""
    rng = random.Random(seed)
    sr = MSR
    n = n_samples(seconds, sr)
    track = [0.0] * (n + n_samples(4.0, sr))
    pd = pad([NOTE[root], NOTE[root] * 1.5, NOTE[root] * 2 * 1.06], seconds, sr, detune=0.008, slow=3)
    for i in range(n):
        track[i] += 0.45 * pd[i]
    beat = 60.0 / bpm
    beats = int(seconds / beat)
    beat = seconds / beats  # snap so the loop is whole beats
    for k in range(beats):
        kick = mul(sweep(95, 40, n_samples(0.22, sr), sr), env_decay(n_samples(0.22, sr), 0.07, sr))
        place(track, kick, k * beat, sr, 0.9 if k % 4 == 0 else 0.55)
        if k % 2 == 1:
            hat = mul(highpass(noise(n_samples(0.05, sr), rng), 3000, sr), env_decay(n_samples(0.05, sr), 0.012, sr))
            place(track, hat, k * beat + beat * 0.5, sr, 0.25)
        if k % 8 in (0, 3, 6):
            ev = bell(NOTE[root] * 2 * (1.5 if k % 8 == 3 else 1.0), 2.5, sr, brightness=0.9)
            place(track, ev, k * beat, sr, 0.22)
    return fold_loop(track, n)


MUSIC = {
    # name: (recipe, gain)
    "title": (lambda: music_ambient(1, "D2", ["D3", "F3", "A3"], ["D5", None, "A4", None, "F4", None, "A4", None], 14.0), 0.9),
    "ashen": (lambda: music_ambient(2, "D2", ["D3", "A3"], ["A4", None, None, "D4", None, "F4", None, None], 12.0), 0.9),
    "warrens": (lambda: music_ambient(3, "E2", ["E3", "G3", "B3" if False else "A3"], [None, "E4", None, "G4", None, None, "A4", None], 12.0, pulse=4), 0.9),
    "cistern": (lambda: music_ambient(4, "C2", ["C3", "G3", "Bb3"], ["G4", "Bb4", None, "C5", None, "G4", None, None], 12.0), 0.9),
    "spire": (lambda: music_ambient(5, "F2", ["F3", "A3", "C4"], ["C5", None, "A4", None, "F4", "A4", None, "C5"], 12.0), 0.9),
    "throne": (lambda: music_ambient(6, "D2", ["D3", "F3", "Bb3"], ["D4", None, "Bb3", None, "A3", None, None, None], 12.0), 0.9),
    "boss_matron": (lambda: music_drive(7, "E2", 12.0, 100), 0.95),
    "boss_bellwarden": (lambda: music_drive(8, "D2", 12.0, 92), 0.95),
}


# ------------------------------------------------------------------ output --


def to_pcm16(x):
    return b"".join(struct.pack("<h", int(max(-1.0, min(1.0, v)) * 32767)) for v in x)


def write_wav(path, x, sr):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with wave.open(path, "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(sr)
        w.writeframes(to_pcm16(x))


def analyse(name, x, sr, loop=False):
    peak = max(abs(v) for v in x)
    rms = math.sqrt(sum(v * v for v in x) / len(x))
    dc = sum(x) / len(x)
    line = f"{name:18s} {len(x) / sr:5.2f}s  peak {peak:.2f}  rms {rms:.3f}  dc {dc:+.4f}"
    problems = []
    if any(v != v for v in x):
        problems.append("NaN")
    if peak > 0.99:
        problems.append("clipping")
    if peak < 0.05:
        problems.append("silent")
    if abs(dc) > 0.02:
        problems.append("DC offset")
    if loop:
        seam = abs(x[0] - x[-1])
        line += f"  seam {seam:.3f}"
        if seam > 0.05:
            problems.append("loop seam")
    return line + ("  <-- " + ", ".join(problems) if problems else ""), problems


def main():
    check = "--check" in sys.argv
    bad = 0
    total_bytes = 0
    for name, (fn, gain) in SFX.items():
        rng = random.Random(hash_name(name))
        x = fade_edges(normalise(fn(rng), gain))
        line, problems = analyse(name, x, SR, loop=(name == "focus"))
        print(line)
        bad += len(problems)
        total_bytes += len(x) * 2
        if not check:
            write_wav(os.path.join(OUT, "sfx", f"{name}.wav"), x, SR)
    for name, (fn, gain) in MUSIC.items():
        x = normalise(fn(), gain * 0.6)
        line, problems = analyse("music/" + name, x, MSR, loop=True)
        print(line)
        bad += len(problems)
        total_bytes += len(x) * 2
        if not check:
            write_wav(os.path.join(OUT, "music", f"{name}.wav"), x, MSR)
    print(f"{len(SFX)} effects + {len(MUSIC)} loops, {total_bytes / 1e6:.1f} MB, {bad} problems")
    if not check:
        print(f"wrote to {os.path.normpath(OUT)}")
    sys.exit(1 if bad else 0)


def hash_name(name):
    h = 2166136261
    for c in name.encode():
        h = ((h ^ c) * 16777619) & 0xFFFFFFFF
    return h


if __name__ == "__main__":
    main()
