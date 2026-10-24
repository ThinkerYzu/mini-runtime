// Minimal stand-in for Firefox's nscore.h: just what the parser core needs.
#pragma once
#include <cstddef>
#include <cstdint>
typedef uint32_t nsresult;
#define NS_OK 0
#define NS_ERROR_OUT_OF_MEMORY 0x8007000eu
#define NS_ERROR_FAILURE 0x80004005u
#define NS_FAILED(r) ((r) != 0)
#define NS_SUCCEEDED(r) ((r) == 0)
#define MOZ_IMPLICIT
#define NS_IMETHOD virtual nsresult
