#!/usr/bin/env bash
# Build silent + narrated exploit-demo recordings from launch slides.
# Narration: macOS `say` (Samantha). Visuals remain readable without audio.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
ASSETS="$ROOT/docs/launch/assets"
cd "$ASSETS"

VOICE="${KOTRO_DEMO_VOICE:-Samantha}"
RATE="${KOTRO_DEMO_VOICE_RATE:-165}"
# Slide durations (seconds). Segment 1 is 15s so the MCP/HTTP-path framing is not clipped.
DUR_1=15
DUR_2=18
DUR_3=18
DUR_4=27
TOTAL_DUR=$((DUR_1 + DUR_2 + DUR_3 + DUR_4))

need() { command -v "$1" >/dev/null || { echo "missing: $1" >&2; exit 1; }; }
need ffmpeg
need say
need ffprobe

for f in slide-01-title.png slide-02-warn.png slide-03-block.png slide-04-dashboard.png; do
  [ -f "$f" ] || { echo "missing slide: $f" >&2; exit 1; }
done

cleanup_intermediates() {
  rm -f \
    narration-01-title.txt narration-02-warn.txt narration-03-block.txt narration-04-dashboard.txt \
    narration-01.aiff narration-02.aiff narration-03.aiff narration-04.aiff \
    narration-01.wav narration-02.wav narration-03.wav narration-04.wav \
    narration-full.wav narration-concat.txt
}
trap cleanup_intermediates EXIT

echo "▶ Building silent video (${TOTAL_DUR}s)"
ffmpeg -y -hide_banner -loglevel error \
  -loop 1 -t "$DUR_1" -i slide-01-title.png \
  -loop 1 -t "$DUR_2" -i slide-02-warn.png \
  -loop 1 -t "$DUR_3" -i slide-03-block.png \
  -loop 1 -t "$DUR_4" -i slide-04-dashboard.png \
  -filter_complex "[0][1][2][3]concat=n=4:v=1:a=0,fps=15,format=yuv420p" \
  -c:v libx264 -preset medium -movflags +faststart \
  exploit-demo-recording-silent.mp4

# Timed narration — on-screen text remains the muted-friendly source of truth.
# Segment 1 deliberately pre-empts the main HN objection (HTTP path ≠ MCP stdio).
cat > narration-01-title.txt <<'EOF'
Kotro is a local firewall for Claude Code and Cursor.
It sits on the HTTP path to the model — not on raw MCP stdio.
Poisoned tool results in the next API call are visible to Kotro.
EOF

cat > narration-02-warn.txt <<'EOF'
Phase A — warn mode, the default.
A poisoned tool result is sent. The request still forwards, but Kotro adds an injection warning header,
and the dashboard increments Injections Detected.
EOF

cat > narration-03-block.txt <<'EOF'
Phase B — hard block with KOTRO_INJECTION_BLOCK set to true.
Same poisoned payload. Kotro returns HTTP 400 — not 403 —
and the dashboard shows the blocked subset.
EOF

cat > narration-04-dashboard.txt <<'EOF'
Here’s the operator dashboard after warm-up plus the block path.
Requests, blocked rows, detected counts, and estimated savings are all coherent —
real counters from a real run. That’s the story for Show HN.
EOF

pad_seg() {
  local idx="$1" dur="$2" text_file="$3"
  local raw="narration-${idx}.aiff"
  local wav="narration-${idx}.wav"
  say -v "$VOICE" -r "$RATE" -f "$text_file" -o "$raw"
  local speech
  speech=$(ffprobe -v error -show_entries format=duration -of default=nw=1:nk=1 "$raw")
  # Leave ~0.4s headroom so atrim never clips the last syllable.
  python3 - "$speech" "$dur" "$idx" <<'PY'
import sys
speech, dur, idx = float(sys.argv[1]), float(sys.argv[2]), sys.argv[3]
headroom = 0.4
if speech > dur - headroom:
    print(
        f"✗ narration-{idx}: speech {speech:.2f}s exceeds slide {dur:.0f}s "
        f"(need ≤ {dur - headroom:.1f}s). Shorten text or raise DUR.",
        file=sys.stderr,
    )
    sys.exit(1)
print(f"  · narration-{idx}: speech {speech:.2f}s / slide {dur:.0f}s ✓")
PY
  ffmpeg -y -hide_banner -loglevel error -i "$raw" \
    -af "apad=whole_dur=${dur},atrim=0:${dur},asetpts=PTS-STARTPTS" \
    -ar 44100 -ac 1 "$wav"
  rm -f "$raw"
}

echo "▶ Synthesizing narration ($VOICE @ ${RATE}wpm)"
pad_seg 01 "$DUR_1" narration-01-title.txt
pad_seg 02 "$DUR_2" narration-02-warn.txt
pad_seg 03 "$DUR_3" narration-03-block.txt
pad_seg 04 "$DUR_4" narration-04-dashboard.txt

echo "▶ Concatenating narration track"
printf "file '%s'\n" narration-01.wav narration-02.wav narration-03.wav narration-04.wav > narration-concat.txt
ffmpeg -y -hide_banner -loglevel error -f concat -safe 0 -i narration-concat.txt -c copy narration-full.wav

echo "▶ Muxing narrated demo"
ffmpeg -y -hide_banner -loglevel error \
  -i exploit-demo-recording-silent.mp4 \
  -i narration-full.wav \
  -c:v copy -c:a aac -b:a 128k -shortest \
  -movflags +faststart \
  exploit-demo-recording.mp4

DUR=$(ffprobe -v error -show_entries format=duration -of default=nw=1:nk=1 exploit-demo-recording.mp4)
echo "✓ silent:   $(du -h exploit-demo-recording-silent.mp4 | awk '{print $1}')"
echo "✓ narrated: $(du -h exploit-demo-recording.mp4 | awk '{print $1}')  (~${DUR}s, target ${TOTAL_DUR}s)"
echo "  Voice: $VOICE  (override with KOTRO_DEMO_VOICE / KOTRO_DEMO_VOICE_RATE)"
# trap cleans intermediates on EXIT
