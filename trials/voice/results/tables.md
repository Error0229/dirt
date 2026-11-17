| Engine and model | Model, MiB | Mistakes: all (human, synthetic) | Shown after the word ended: median (9 in 10) | Changed, per 100 words | Speed, of real time | Worst lag | Memory, MiB | Load |
|---|---|---|---|---|---|---|---|---|
| sherpa-onnx, zipformer-en-20M (8-bit) | 42 | 24.3% (22.3, 27.6) | 0.51 s (0.72 s) | 0 | 0.05 | 0.0 s | 113 | 1.6 s |
| sherpa-onnx, zipformer-en-2023-06-26 (8-bit) | 68 | 4.8% (1.7, 9.7) | 0.61 s (0.83 s) | 0 | 0.10 | 0.0 s | 163 | 7.9 s |
| sherpa-onnx, Kroko English | 68 | 2.4% (3.9, 0.0) | 1.08 s (1.64 s) | 0 | 0.05 | 0.1 s | 182 | 4.9 s |
| sherpa-onnx, NeMo fast conformer 480 ms (8-bit) | 131 | 3.7% (2.6, 5.5) | 0.75 s (1.02 s) | 0 | 0.13 | 0.0 s | 227 | 4.9 s |
| sherpa-onnx, Moonshine tiny read again every 0.3 s | 42 | 7.0% (7.9, 5.5) | 0.23 s (0.63 s) | 124 | 0.63 | 0.8 s | 174 | 1.9 s |
| sherpa-onnx, Moonshine tiny read again every 0.5 s | 42 | 5.6% (5.7, 5.5) | 0.31 s (0.67 s) | 77 | 0.33 | 0.7 s | 173 | 1.3 s |
| sherpa-onnx, Moonshine base read again every 0.5 s | 135 | 3.5% (3.1, 4.1) | 0.39 s (0.80 s) | 89 | 0.56 | 1.2 s | 358 | 1.6 s |
| whisper.cpp, `tiny.en`, full window | 74 | 4.3% (5.2, 2.8) | 2.25 s (3.47 s) | 13 | 1.16 | 3.0 s | 243 | 0.4 s |
| whisper.cpp, `tiny.en`, short window | 74 | 5.1% (6.6, 2.8) | 0.55 s (1.44 s) | 29 | 0.83 | 2.5 s | 243 | 0.3 s |
| whisper.cpp, `base.en`, short window | 141 | 2.1% (3.1, 0.7) | 0.74 s (1.75 s) | 90 | 1.03 | 4.0 s | 356 | 0.7 s |
| whisper.cpp, `base.en` 5-bit, short window | 57 | 3.5% (5.2, 0.7) | 0.73 s (1.84 s) | 31 | 1.01 | 3.5 s | 272 | 0.3 s |
| whisper.cpp, `small.en`, short window | 465 | 1.3% (1.7, 0.7) | 2.92 s (5.73 s) | 14 | 1.48 | 6.7 s | 795 | 1.5 s |

| Model | Model, MiB | Mistakes | Time for one thought, median |
|---|---|---|---|
| Moonshine tiny (8-bit), through sherpa-onnx | 42 | 2.4% | 0.20 s |
| Moonshine base (8-bit), through sherpa-onnx | 135 | 2.1% | 0.33 s |
| whisper.cpp, `tiny.en` | 74 | 4.0% | 1.64 s |
| whisper.cpp, `base.en` 5-bit | 57 | 1.3% | 3.23 s |
| whisper.cpp, `base.en` | 141 | 1.1% | 3.37 s |
| whisper.cpp, `small.en` | 465 | 1.3% | 11.89 s |

| | After the last word: median (worst) |
|---|---|
| Silero's detector, 22 thoughts | 1.75 s (2.47 s) |
| recogniser's rule, zipformer-en-2023-06-26 | 2.32 s (2.58 s) |
| recogniser's rule, Kroko | 2.35 s (3.76 s) |
| recogniser's rule, NeMo | 2.23 s (2.60 s) |

calibration, s: [0.08, 0.1, 0.11, 0.12, 0.13, 0.14, 0.15, 0.16, 0.17, 0.19]
