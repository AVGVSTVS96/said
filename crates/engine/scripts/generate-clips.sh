#!/usr/bin/env bash
set -euo pipefail
cache="$HOME/.cache/voice-models/engine-proof"
mkdir -p "$cache"
case "$(uname -s)" in Darwin) ;; *) printf 'Generate on macOS with its built-in say command.\n' >&2; exit 1 ;; esac
for entry in \
 'negation|No no please do not delete the file. Keep every word.' \
 'repair|Um uh I I want the blue one. Actually no the red one.' \
 'repeats|Go go go. No no no. Yes yes yes.'; do
 name="${entry%%|*}"
 say -v Samantha -o "$cache/$name.aiff" "${entry#*|}"
 ffmpeg -v error -y -i "$cache/$name.aiff" -ar 16000 -ac 1 -c:a pcm_s16le "$cache/$name.wav"
done
ffmpeg -v error -y -f lavfi -i anullsrc=r=16000:cl=mono -t 2 -c:a pcm_s16le "$cache/silence.wav"
ffmpeg -v error -y -f lavfi -i anullsrc=r=16000:cl=mono -t 25 -c:a pcm_s16le "$cache/boundary-gap.wav"
ffmpeg -v error -y -i "$cache/repeats.wav" -i "$cache/boundary-gap.wav" \
 -filter_complex '[0:a]asplit=2[a][b];[a][1:a][b]concat=n=3:v=0:a=1[out]' \
 -map '[out]' -c:a pcm_s16le "$cache/boundaries.wav"
