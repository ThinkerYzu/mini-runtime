// Every static atom as nsGkAtoms::name, from the list in nsGkAtomList.h (copied from a Firefox build).
#pragma once
#include "nsAtom.h"

namespace mrt_detail {
enum AtomIndex : uint32_t {
#define GK_ATOM(name, str) k_##name,
#include "nsGkAtomList.h"
#undef GK_ATOM
  kAtomCount
};
extern nsAtom gStaticAtoms[kAtomCount];
}  // namespace mrt_detail

namespace nsGkAtoms {
#define GK_ATOM(name, str) inline nsAtom* const name = &mrt_detail::gStaticAtoms[mrt_detail::k_##name];
#include "nsGkAtomList.h"
#undef GK_ATOM
}  // namespace nsGkAtoms
