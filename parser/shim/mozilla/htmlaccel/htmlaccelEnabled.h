// Firefox has an optional SIMD fast path in the tokenizer. This runtime always uses the plain path.
#pragma once
namespace mozilla::htmlaccel {
inline bool htmlaccelEnabled() { return false; }
}  // namespace mozilla::htmlaccel
