#!/usr/bin/env python3
"""Write single-speaker cases for `cargo xtask diar-eval`.

One remote voice should come out as one speaker. These cases show when the
turn finder or the clustering splits it:

  single_monologue   one long stretch of one voice, barely pausing
  single_varied      one voice, with sentences that vary in length and pace

Run on macOS (it uses `say` and `afconvert`). Writes the audio, RTTM and a
`manifest.json` into the directory given, which should be outside the repo: no
audio or RTTM is committed. TTS is not real-call evidence.

  python3 scripts/gen_diar_cases.py /tmp/diar-cases --voice Samantha
  cargo xtask diar-eval /tmp/diar-cases/manifest.json

Read `speakers=found/1`: more than one found is the voice being split.
"""

import argparse
import json
import pathlib
import subprocess
import tempfile
import wave

SAMPLE_RATE = 16_000
DEFAULT_VOICE = "Samantha"
DEFAULT_RATE = 170
# A pause this short sits inside one reference turn: it merges in VAD anyway.
SAME_TURN_GAP_MS = 300

PARAGRAPHS = [
    "When we started this project the main goal was to reduce the time it takes "
    "to prepare the quarterly numbers, and most of the early work went into "
    "understanding where that time was actually going.",
    "It turned out that the slowest part was not the calculation at all but the "
    "back and forth between teams, because every group kept its own copy of the "
    "spreadsheet and nobody could say which one was current.",
    "So the first thing we changed was to agree on a single source, which sounds "
    "simple, but it took about six weeks of conversations before everyone was "
    "comfortable giving up their own version of the figures.",
    "Once that was settled the rest moved quickly, and by the end of the summer "
    "the whole process took two days instead of two weeks, which freed people up "
    "to look at what the numbers meant instead of arguing about them.",
    "There is still more to do, particularly around how we handle late changes, "
    "and I would like to spend the next few months making sure the improvements "
    "we made stick when the team grows and new people join.",
]

# (text, words per minute, ms of pause after it)
VARIED = [
    ("Right.", 170, 900),
    ("Let me start with where we are today.", 190, 500),
    (
        "We have forty two customers on the new plan, up from thirty one at the "
        "start of the quarter, and churn is lower than any of us expected.",
        160,
        700,
    ),
    ("Does that match what you are seeing?", 210, 1200),
    ("Okay.", 150, 600),
    ("Yes, exactly.", 200, 800),
    (
        "The part I am less sure about is the pricing page, because the test ran "
        "for only nine days and the traffic that week was unusual.",
        175,
        400,
    ),
    ("Hmm, I wonder.", 140, 1000),
    ("Could we run it again in March?", 205, 500),
    (
        "If we do, I would change one thing, which is to keep the old page live "
        "for existing accounts, so that support does not get a wave of questions "
        "from people who think their bill has changed.",
        185,
        900,
    ),
    ("Good.", 170, 1500),
    ("Next, the hiring plan.", 195, 300),
    (
        "We budgeted for three engineers and one designer, we have made two offers, "
        "and I expect to hear back on both of them by the end of next week.",
        165,
        700,
    ),
    ("That is really all I have.", 180, 600),
    ("Any questions before we move on?", 200, 0),
]


def monologue():
    # Long enough that the voice has time to drift, with only short breaths.
    return [(text, DEFAULT_RATE, 150) for text in PARAGRAPHS * 3]


CASES = {"single_monologue": monologue, "single_varied": lambda: VARIED}


def synthesize(text, voice, rate, work):
    """16 kHz mono 16-bit PCM of `text`, as bytes."""
    aiff = work / "clip.aiff"
    wav = work / "clip.wav"
    subprocess.run(
        ["say", "-v", voice, "-r", str(rate), "-o", str(aiff), text], check=True
    )
    subprocess.run(
        ["afconvert", "-f", "WAVE", "-d", f"LEI16@{SAMPLE_RATE}", "-c", "1", str(aiff), str(wav)],
        check=True,
    )
    with wave.open(str(wav), "rb") as clip:
        return clip.readframes(clip.getnframes())


def layout(clips, speech):
    """The track's PCM and reference turns, from `clips` and `speech(clip)`."""
    pcm = bytearray()
    turns = []
    for clip in clips:
        audio = speech(clip)
        t0 = len(pcm) // 2 * 1000 // SAMPLE_RATE
        pcm += audio
        t1 = len(pcm) // 2 * 1000 // SAMPLE_RATE
        if turns and t0 - turns[-1][1] <= SAME_TURN_GAP_MS:
            turns[-1][1] = t1
        else:
            turns.append([t0, t1])
        pcm += bytes(2 * clip[2] * SAMPLE_RATE // 1000)
    return bytes(pcm), turns


def rttm(name, speaker, turns):
    return "".join(
        f"SPEAKER {name} 1 {t0 / 1000:.3f} {(t1 - t0) / 1000:.3f} <NA> <NA> {speaker} <NA> <NA>\n"
        for t0, t1 in turns
    )


def write_wav(path, pcm):
    with wave.open(str(path), "wb") as out:
        out.setnchannels(1)
        out.setsampwidth(2)
        out.setframerate(SAMPLE_RATE)
        out.writeframes(pcm)


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n", 1)[0])
    parser.add_argument("out", type=pathlib.Path)
    parser.add_argument("--voice", default=DEFAULT_VOICE)
    args = parser.parse_args()
    args.out.mkdir(parents=True, exist_ok=True)
    meetings = []
    with tempfile.TemporaryDirectory() as tmp:
        work = pathlib.Path(tmp)
        for name, clips in CASES.items():
            pcm, turns = layout(
                clips(),
                lambda clip: synthesize(clip[0], args.voice, clip[1], work),
            )
            write_wav(args.out / f"{name}.wav", pcm)
            (args.out / f"{name}.rttm").write_text(rttm(name, "remote", turns))
            meetings.append(
                {"name": name, "system": f"{name}.wav", "ref": f"{name}.rttm"}
            )
            print(f"{name}: {len(pcm) / 2 / SAMPLE_RATE:.1f} s, {len(turns)} reference turn(s)")
    (args.out / "manifest.json").write_text(json.dumps({"meetings": meetings}, indent=2) + "\n")


if __name__ == "__main__":
    main()
