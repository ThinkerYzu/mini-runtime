// Logging is not part of this runtime.
#pragma once
namespace mozilla {
enum class LogLevel { Disabled, Error, Warning, Info, Debug, Verbose };
struct LazyLogModule {
  constexpr LazyLogModule(const char*) {}
};
}  // namespace mozilla
#define MOZ_LOG(m, l, a) ((void)0)
#define MOZ_LOG_TEST(m, l) false
