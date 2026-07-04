# Bench fixtures

## jfk.wav

An 11 s clip of John F. Kennedy's 1961 inaugural address ("ask not what your country can do for you..."), 16 kHz mono 16-bit PCM.

- Real recorded human speech, as spike 3 requires for graded fixtures (synthesized `say` audio is forbidden).
- Public domain: a work of the United States federal government.
- Same canonical sample whisper.cpp ships in `samples/jfk.wav`, so numbers are comparable to upstream whisper.cpp benchmarks.

Baselines for the CI regression gate live in `../baselines/<runner>.json`; mint one by committing the JSON line the bench prints.
