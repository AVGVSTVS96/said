#include <cassert>
#include <cmath>
#include <vector>
#include "voice-activity-detector.h"

int main() {
  for (size_t block : {1, 511, 1600, 8000, 32000}) {
    VoiceActivityDetector vad(0.0f, 16, 512, 8192, 15 * 16000);
    vad.start();
    std::vector<float> input(181 * 16000 + 37);
    for (size_t i = 0; i < input.size(); ++i) {
      input[i] = std::sin(static_cast<float>(i) / 137.0f) * 0.1f;
    }
    for (size_t i = 0; i < input.size(); i += block) {
      vad.process_audio(input.data() + i, std::min(block, input.size() - i), 16000);
    }
    vad.deactivate();
    vad.flush(nullptr, 0, 16000);
    std::vector<float> output;
    float end = 0.0f;
    const auto &segments = *vad.get_segments();
    assert(segments.size() >= 12);
    for (const auto &segment : segments) {
      assert(segment.is_complete);
      assert(segment.audio_data.size() <= 15 * 16000);
      assert(std::abs(segment.start_time - end) < 0.0001f);
      end = segment.end_time;
      output.insert(output.end(), segment.audio_data.begin(), segment.audio_data.end());
    }
    assert(output.size() >= input.size() && output.size() < input.size() + 512);
    assert(std::equal(input.begin(), input.end(), output.begin()));
    for (size_t i = input.size(); i < output.size(); ++i) assert(output[i] == 0.0f);
  }
}
