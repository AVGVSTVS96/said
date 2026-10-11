# Corpus licenses and provenance

The benchmark implementation and provisioning scripts use the repository's MIT
license. Corpus data has separate licenses recorded per clip.

## AMI excerpts (CC BY 4.0)

Credit: **AMI Consortium, AMI Meeting Corpus**, meeting ES2002a,
manual annotations v1.6.2. Five cropped individual-headset excerpts include
speakers A, B, C and D. Wave headers are rewritten around the selected 16 kHz
PCM; audio samples are unchanged. Word XML is joined into the literal references
as documented in README. Crop intervals, sources and hashes are in
`recipes.jsonl`, `clips.jsonl` and `sources.json`.

The [official download page](https://groups.inf.ed.ac.uk/ami/download/)
explicitly places signals, transcription and released annotations under
[CC BY 4.0](https://creativecommons.org/licenses/by/4.0/).
Its annotation release changed the license to CC BY 4.0 on 10 April 2017.
Earlier AMI papers describe an older license; this corpus uses the 2017 release.
The [transcription manual](https://groups.inf.ed.ac.uk/ami/corpus/Guidelines/speech-transcription-manual.v1.2.pdf)
documents hesitation and disfluency transcription.

Suggested academic attribution: Jean Carletta et al.,
[The AMI Meeting Corpus: A Pre-announcement (2006)](https://doi.org/10.1007/11677482_3).
AMI is spontaneous meeting speech, including repeated words and unfinished
fragments; its headset channels still capture other speakers. These excerpts
are a small diagnostic selection, not the official AMI evaluation split.

## Synthetic prompts and controls (CC0 1.0)

The newly authored scripts, their generated WAVs, and deterministic silence/noise
controls in this benchmark are dedicated to the public domain under
[CC0 1.0](https://creativecommons.org/publicdomain/zero/1.0/).
No private prompts, names, histories or voice recordings supplied their content.

TTS generation runs locally with
[OHF-Voice Piper](https://github.com/OHF-Voice/piper1-gpl) 1.3.0 (GPL-3.0),
invoked as a development tool, never as an application recognizer dependency.
The voice is
[rhasspy/piper-voices en_US-ljspeech-high](https://huggingface.co/rhasspy/piper-voices/tree/c10ece1aade47bb51c153c893d14e5bf8e5b7117/en/en_US/ljspeech/high)
(the model repository is MIT). The
[voice model card](https://huggingface.co/rhasspy/piper-voices/blob/c10ece1aade47bb51c153c893d14e5bf8e5b7117/en/en_US/ljspeech/high/MODEL_CARD)
says it was trained from scratch on public-domain
[LJ Speech recordings](https://keithito.com/LJ-Speech-Dataset/).
The model/config SHA-256 hashes and immutable repository revision are pinned.
We distribute generated audio, not Piper or its native dependencies.

## Other corpus research

[ICSI](https://groups.inf.ed.ac.uk/ami/icsi/license.shtml) also publishes meeting
speech and annotations under CC BY 4.0 and is suitable for future expansion.
[LibriTTS](https://www.openslr.org/60/) is CC BY 4.0 but primarily read audiobook
speech, so it does not supply the spontaneous disfluency tests required here.
No audio from those corpora is included. The Lessac Piper voice was not selected:
its model card points at a research-only source-data agreement, whereas the
LJSpeech card states public-domain data and training from scratch.
