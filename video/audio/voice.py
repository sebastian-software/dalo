"""Generate voiceover takes with Chatterbox.

The voice is conditioned on a synthetic reference spoken by Kokoro's
"af_bella" voice, so no real person's voice is cloned. Every take is written
to <work>/takes/<line>_s<seed>.wav; existing takes are kept. Pick takes by ear
and name them in a cue sheet.

usage: voice.py <work> <lines.json> [line ...] [--seeds 1,2,3]
"""
import json, os, sys

import numpy as np
import soundfile as sf
import torch

work, lines_path, *rest = sys.argv[1:]
seeds = [7, 11, 23]
if "--seeds" in rest:
    i = rest.index("--seeds")
    seeds = [int(s) for s in rest[i + 1].split(",")]
    rest = rest[:i] + rest[i + 2 :]
lines = json.load(open(lines_path))
wanted = rest or list(lines)
os.makedirs(os.path.join(work, "takes"), exist_ok=True)

ref = os.path.join(work, "reference.wav")
if not os.path.exists(ref):
    from phonemizer.backend.espeak.wrapper import EspeakWrapper
    import misaki.espeak  # noqa: F401  (sets a data path that is too long)

    EspeakWrapper.set_data_path(os.path.join(work, "espeak-ng-data"))
    from kokoro import KPipeline

    text = (
        "Today, we want to show you something we have been working on for a long time. "
        "It is simple, it is calm, and it just works. Let me walk you through it."
    )
    pipe = KPipeline(lang_code="a")
    audio = [a.numpy() for _, _, a in pipe(text, voice="af_bella", speed=0.95)]
    sf.write(ref, np.concatenate(audio), 24000)

from chatterbox.tts import ChatterboxTTS

model = ChatterboxTTS.from_pretrained(device="mps" if torch.backends.mps.is_available() else "cpu")
model.prepare_conditionals(ref, exaggeration=0.45)
for key in wanted:
    for seed in seeds:
        path = os.path.join(work, "takes", f"{key}_s{seed}.wav")
        if os.path.exists(path):
            continue
        torch.manual_seed(seed)
        wav = model.generate(lines[key], exaggeration=0.45, cfg_weight=0.4, temperature=0.7)
        sf.write(path, wav.squeeze(0).cpu().numpy(), model.sr)
        print(key, seed, flush=True)
