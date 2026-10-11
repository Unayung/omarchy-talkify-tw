// Talkify's Speech Model (Breeze ASR 25) and its text rules, for apps that
// are not the Linux daemon. Build: cargo build --release -p talkify-ffi
// --features metal, then link target/release/libtalkify.a.
#ifndef TALKIFY_H
#define TALKIFY_H

#include <stdbool.h>
#include <stddef.h>

typedef struct TalkifySpeech TalkifySpeech;

// Loads the model's weights (a whisper.cpp GGML file). NULL when it cannot.
// Loading takes about a second; keep the result for the app's lifetime.
TalkifySpeech *talkify_open(const char *weights_path);

// One finished dictation: 16 kHz mono samples in, text out, after the Tidy
// pass and the Vocabulary.
//
//   vocabulary    the words file's text, or NULL for the seed vocabulary.
//                 Its words are also the model's hints.
//   send_phrases  "送出, send it", or NULL when a spoken send must stay text.
//
// Returns JSON, {"text": "...", "raw": "...", "send": false}, to be freed
// with talkify_free. "raw" is what was heard before the Tidy pass; "send" is
// true when a send phrase was said last, after a pause, and removed.
char *talkify_dictate(TalkifySpeech *speech, const float *samples, size_t count,
                      const char *vocabulary, const char *send_phrases);

void talkify_free(char *text);
void talkify_close(TalkifySpeech *speech);

#endif
