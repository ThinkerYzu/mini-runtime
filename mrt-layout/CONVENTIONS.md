# Translation conventions (mrt-layout)

Translation approach: adapt code from Firefox where permitted by its license, and keep the translation
in the same shape as Gecko's C++ so Gecko's changes are easy to catch up with. Everything under
`src/` is a translation of a Firefox file unless it says otherwise.

Reference tree: the Firefox checkout selected by `FIREFOX_SRC` or a tool's `--tree` option.
Base revision: `GECKO_REV`. Keep reference source separate from this repository.

## Files and modules

- `src/<dir>/<File>.rs` translates `<dir>/<File>.h` and `<File>.cpp` of the Firefox tree, header and
  source together, in the order of the header's declarations with the .cpp bodies where the header
  has only declarations. Directories mirror the Firefox tree (`src/gfx/src/nsRect.rs` is `gfx/src/nsRect.h`).
- `src/lib.rs` declares every file with `#[path = "..."] mod ...;` and re-exports everything with
  `pub use X::*;`. Every translated file starts with `use crate::*;`. This stands in for C++
  `#include` plus `using namespace mozilla`: namespaces (`mozilla::`, `mozilla::gfx::`) are flattened.
- Every file has `#![allow(...)]`-free naming: lib.rs allows `non_snake_case`, `non_camel_case_types`,
  `non_upper_case_globals`, `dead_code`, `unused` for the whole crate.

## Names

- Types, functions, members, constants, enum variants, macros keep their exact C++ spelling
  (`nsFloatManager`, `GetFlowArea`, `mFloats`, `nscoord_MAX`, `eSideTop`, `LogicalRect`).
- Member functions are `impl` methods with `&self`/`&mut self`; static functions are associated
  functions without `self`. Parameters keep their names (`aBCoord`), locals keep theirs.
- **Overloads.** Rust has no overloading. A name with several overloads gets, on every one of them,
  the suffix `__` followed by the parameter type names joined by `_` (`Inflate__nscoord`,
  `Inflate__nscoord_nscoord`, `Inflate__nsMargin`); an overload without parameters gets a bare `__` (`LineLeft__()` next to `LineLeft__ShapeType_nscoord_nscoord(..)`). A name with one definition keeps the plain name.
  Constructors: the all-members one is `new(...)`; other constructors are `new__<param types>`;
  the default constructor is `Default::default()` (also `new()` when C++ has one).
- **Templates and CRTP** (`BaseRect<T, Sub, ...>` subclassed by `nsRect`): Rust has no inheritance, so
  the base class is a macro in the base file (`BaseRect.rs`: `macro_rules! BaseRect`) that is expanded
  inside the subclass's file (`nsRect.rs`) to define the members and methods on the subclass. Only the
  instantiations the layout code needs exist (`nscoord` ones).
- **Operators** become the std traits (`Add`, `Sub`, `AddAssign`, `Neg`, `Mul`, `Div`, `PartialEq`)
  with the same meaning. A deleted or private operator is not implemented.
- `T*`/`T&` parameters: small `Copy` types (nscoord, nsPoint, nsSize, nsRect, WritingMode, Logical*)
  are passed by value; out-parameters (`nscoord& aX`) are `&mut`; objects with identity or large state
  are `&`/`&mut`. Pointers to frames are `Option<nsIFrame>` handles (see below) or `nsIFrame`.
- `UniquePtr<T>` is `Option<Box<T>>`; `nsTArray<T>` and `AutoTArray` are `Vec<T>`; `nsTHashSet` is
  `HashSet`. Their method names are kept where Rust has an equivalent (`Length()`, `IsEmpty()`,
  `AppendElement`, `TruncateLength`) via thin wrappers in `xpcom/nsTArray.rs` if the translated code
  uses them; otherwise write the Rust idiom on that line and keep the C++ line as a comment above it.

- **Inheritance.** A C++ subclass is a struct that holds its base class in a field and `Deref`s (and
  `DerefMut`s) to it, so inherited members and methods are used as `self.mFrame`, `self.ComputedMargin()`
  as in C++ (`ReflowInput : SizeComputationInput`). A call that names the base class explicitly
  (`nsContainerFrame::ComputeAutoSize(...)`) is a method named `<Class>_<Name>`
  (`nsContainerFrame_ComputeAutoSize`); the virtual call itself is the plain name and is a `match` on the
  frame type (see the adapter layer below).
- **Nested types** are top-level, named `Outer_Inner` (`ReflowInput_InitFlag`, `nsIFrame_AspectRatioUsage`).
- **Pointers that Gecko shares and replaces** (`nsFloatManager*` in a ReflowInput, `nsIFrame**`) are raw
  pointers; a `const ReflowInput*` link to a parent is `&'a ReflowInput<'a>`.
- **C++ templates over style types** (`template <typename SizeOrMaxSize>`) are generic functions over the
  trait `StyleSizeLike` (ServoStyleConsts.rs). A C++ generic lambda is a nested generic `fn` that takes
  its captures as parameters.

## Order, comments, markers

- Functions appear in the order of the C++ file. The comments are copied as they are (including the
  line structure); a comment is changed only to say what the Rust differs in.
- Each translated function or struct has, on the line above it:
  `// Gecko: <path of the .cpp or .h> <qualified name>` (for example
  `// Gecko: layout/generic/nsFloatManager.cpp nsFloatManager::GetFlowArea`). The revision is the
  file-level `GECKO_REV`; a file translated from another revision says so in its first line:
  `// Gecko-Rev: <sha>`. `tools/gecko-drift.py` reads these markers.
- Code that is left out stays in place as a stub, with the original code as a comment and
  `// NOT PORTED: <reason>`; for a block longer than about 50 lines the comment names the file and
  line range at `GECKO_REV` instead of copying it; a function that is called but not ported has a body of
  `unimplemented!("NOT PORTED: <reason>")`. Nothing is silently deleted.
- Debug-only code (`#ifdef DEBUG`) is `#[cfg(debug_assertions)]`. Reference-count logging
  (`MOZ_COUNT_CTOR`) is dropped with a one-line `// NOT PORTED: refcount logging` comment per file.

## Assertions and errors

`MOZ_ASSERT!`, `MOZ_ASSERT_UNREACHABLE!`, `NS_ASSERTION!`, `NS_WARNING!`, `NS_WARNING_ASSERTION!`,
`MOZ_CRASH!` exist (src/mfbt/Assertions.rs) with the C++ argument order and map to `debug_assert!`,
`eprintln!` and `panic!`. `nsresult` is `u32` with `NS_OK = 0` (src/xpcom/nsError.rs).

## Frames and styles (the adapter layer)

`nsIFrame` is a `Copy` handle (a frame id) in `layout/generic/nsIFrame.rs`. Its methods are Gecko's
(`GetParent()`, `GetNextSibling()`, `GetRect()`, `GetSize()`, `GetNormalPosition()`,
`GetWritingMode()`, `StyleDisplay()`, `GetProperty(...)`) and read the flat frame arrays through a
thread-local frame tree, so translated bodies read like the original. `StyleDisplay()` returns a
wrapper over Stylo's computed `Box` struct with Gecko's `nsStyleDisplay` method names
(`UsedFloat`, `UsedClear`, ...). Anything that is virtual in C++ is a `match` on a frame-type tag in
the adapter, with a comment.

**Storage follows Gecko's layout** (memory is compared with Firefox's, `bench/compare_memory.py`): a frame's
data is `FrameData` in chunks that never move (`FrameStore`), so `nsContainerFrame::mFrames` and
`nsInlineFrame::mBaseline` are inline members whose address is stable; `mContent` is the node id;
frame properties are the sparse `FrameProperties` (`layout/base/FrameProperties.rs`, 8 bytes while
empty); overflow areas are `mOverflow` (none, four small deltas, or `OverflowAreasProperty`), with
`SetRect` re-packing them as Gecko's does; the members of text frames are 32 bytes in a dense table
(the rare ones are properties). A new per-frame member goes behind a Gecko-named accessor in one of
these, not in a per-frame `HashMap`. `ShrinkFrameStorage` (driver.rs) gives back spare capacity after reflow.

## Tests

Where the Firefox tree has a gtest (`layout/generic/gtest`, `gfx/tests/gtest`), translate it next to
the file as a `#[cfg(test)] mod tests` with the same test names. Otherwise write tests from the
documented behaviour, and say so.

## From a page to frames (the driver)

`src/layout/base/nsCSSFrameConstructor.rs` (reduced) builds the frames of a document from the arena DOM
and Stylo's computed styles; `src/layout/base/PresShell.rs` reflows them. `src/driver.rs` (not a Firefox
file) is the glue: `LayoutDocument(&StyleDom, width_px, height_px)` = `ConstructFrames` + `ReflowFrames`.
`src/bin/layoutcount.rs` is the command line (`layoutcount page.html [--tree[=N]] [--ids]`).

- **Crate dependency:** `mrt-layout` depends on `mrt-style`, which depends on `mrt-dom`, as it already did for the
  style adapters. mrt-style and mrt-dom know nothing of layout, so there is no cycle, and no new crate was
  needed. The frame constructor reads the DOM and the styles through a `&StyleDom` (`ContentId` is a node id of
  the arena; the content a frame holds, `nsIContent`, is a `Copy` handle of the node (the node id, or a
  synthetic content for generated content), as Gecko's `mContent` is one pointer; its methods read the node
  from the arena DOM, which `nsCSSFrameConstructor::new` makes reachable through a thread-local, so the
  `StyleDom` must outlive the frames: see `nsIContent` in `nsIFrame.rs` and `KeepDocumentAlive` in `driver.rs`).
- **Styles:** a frame's `mStyle` is Stylo's `ComputedValues` Arc, so `Style*()` read the real computed values.
  Text and other anonymous boxes get `StyleDom::anonymous_style` (inherited properties of the parent, the
  initial values for the rest, the given display); the frame constructor caches one per parent style.
- **One page per thread:** the frame tree and the layout stubs are thread-local, so a program lays out one page
  per thread (the tests do this by default; `layoutcount` spawns a thread with a big stack).
- **Stand-ins for frame classes that are not ported** are block frames with the display of the class that
  does the same work: grid containers (block), replaced elements and form controls (leaf block,
  inline-block when inline-level). Each is a `// NOT PORTED:` comment where Gecko has the real thing.
- **Absolutely and fixed positioned boxes** are Gecko's (`AbsoluteContainingBlock.rs`, session 54): a
  frame in its containing block's absolute list and a placeholder in the flow. The viewport and the canvas
  are reflowed with an unconstrained block-size (the document is measured, not scrolled), so their
  containing blocks take the visible area's block-size, as Gecko's root scroll frame would give them
  (`ViewportFrame::Reflow`, `nsCanvasFrame::Reflow`). Not ported: positioned table parts as containing
  blocks, the top layer, anchor positioning, abspos fragmentation. Flex containers are real (`nsFlexContainerFrame`; the constructor
  wraps runs of text and line breaks in anonymous flex items with `CreateNeededAnonFlexOrGridItems`, styled
  as `::-moz-anonymous-item`, display: block). Tables are real: the constructor builds the table wrapper, table,
  row groups, rows, cells, columns and captions, with the anonymous parents of CSS 2.2 17.2.1
  (CreateNeededPseudoContainers).
- **Table anonymous boxes and attributes.** `ResolveInheritingAnonymousBoxStyle` and
  `ResolveNonInheritingAnonymousBoxStyle` make the styles of the table anonymous boxes by hand (the rules of
  ua.css, since Stylo's servo configuration has no such pseudo-elements); `SetTableAnonBoxStyleResolver` is
  installed by the constructor for the table's synthetic columns. `colspan`, `rowspan` and `span` are read
  from the DOM by the constructor and given to the frames (`SetSpans`, `SetSpan`). `cellpadding`,
  `cellspacing`, `border`, `align`, `valign`, `nowrap` and the sizes of table parts are presentational hints
  in `mrt-style` (`synthesize_presentational_hints_for_legacy_attributes`), the way Gecko's mapped attributes
  are, and html.css's `:-moz-table-border-nonzero` is the servo `:-servo-nonzero-border`.
