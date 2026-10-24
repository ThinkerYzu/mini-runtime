// The C interface between the Rust side of mrt-js and the C++ layer over SpiderMonkey.
//
// Rust calls the mrt_js_* functions. The native functions of the DOM bindings call back into Rust
// through the mrt_dom_* functions, which Rust defines; `host` is the Rust side's page state,
// passed through untouched.
#pragma once
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct MrtJs MrtJs;

// Bytes of one realm or zone, as JS::ServoSizes groups them.
typedef struct MrtJsSizes {
  size_t gc_used, gc_unused, gc_admin, malloc_heap, non_heap;
} MrtJsSizes;

typedef struct MrtJsMemory {
  MrtJsSizes realm;  // Firefox reports this under the window: js-realm(...)
  MrtJsSizes zone;   // and this under its top-level entry: js-zone(...)
  // Free and admin bytes of the zone's GC buffer allocator, which Firefox does not report under the
  // window (and which `zone` leaves out to match).
  size_t zone_buffer_slack;
  // The wrapper cache of the C++ layer: 8 bytes per node id up to the highest one wrapped.
  size_t wrapper_cache;
  size_t reflectors, preserved;
} MrtJsMemory;

// The page's global as Gecko makes a content window's: its document's origin ("null" for an
// opaque one), whether it is a secure context, and whether it is cross-origin isolated (which
// decides the SharedArrayBuffer constructor). Firefox's answers for the page when it has a bridge.
typedef struct MrtGlobalOptions {
  const char* origin;
  size_t origin_len;
  int secure_context, cross_origin_isolated;
} MrtGlobalOptions;

// One context and one global per thread; the first call also initializes SpiderMonkey.
MrtJs* mrt_js_new(void* host, const MrtGlobalOptions* page);
// A frame's global, in the zone and compartment Gecko's SelectZone gives an in-process frame of
// the page: the page's zone, and a same-origin compartment of it when there is one. Frames are not
// run yet (stage 6 phase 3); this is the rule and its test. Returns the frame's index, -1 on failure.
int32_t mrt_js_new_frame_global(MrtJs* js, const MrtGlobalOptions* frame);
// For frame `i`: 1 in the page's zone, 2 in the page's compartment, 4 a secure context, 8 in the
// compartment of an earlier frame. -1 for a bad index.
int32_t mrt_js_frame_global_relation(MrtJs* js, int32_t i);
void mrt_js_free(MrtJs* js);
// After every context is freed, before the process exits (SpiderMonkey's helper threads hold locks
// that its static destructors would otherwise destroy).
void mrt_js_shutdown(void);
// Run a script; false if it threw (the exception is reported to stderr and counted).
int mrt_js_eval(MrtJs* js, const char* src, size_t len, const char* filename);
// Like mrt_js_eval, and the script's value as a number in *out (NaN when it is not a number).
int mrt_js_eval_number(MrtJs* js, const char* src, size_t len, const char* filename, double* out);
// A driver script's value as UTF-8 text, valid until the next call.
const char* mrt_js_eval_string(MrtJs* js, const char* src, size_t len, const char* filename, size_t* out_len);
size_t mrt_js_errors(MrtJs* js);
void mrt_js_gc(MrtJs* js);
// The page going idle: Firefox's shrinking GC when the user is inactive (the nursery shrunk, script
// sources compressed). Returns the milliseconds spent waiting for the compressions.
size_t mrt_js_idle_gc(MrtJs* js);
// One shrinking GC, without the wait (an interactive page idle a while).
void mrt_js_shrink_gc(MrtJs* js);
void mrt_js_memory(MrtJs* js, MrtJsMemory* out);
// The whole runtime's memory as one JSON object (bytes) in `out`, up to `cap` bytes with the NUL;
// returns the length the whole object needs (realsite's report, GAPS A16).
size_t mrt_js_memory_detail(MrtJs* js, char* out, size_t cap);
// Heap bytes of the page's sessionStorage (all origins), which is page state.
size_t mrt_js_session_storage_bytes(MrtJs* js);

// Defined in Rust (src/host.rs). Strings passed out are UTF-8 and stay valid until the next call.
int32_t mrt_dom_kind(void* host, uint32_t id);
// 0 parent 1 first 2 last 3 next 4 prev; 5 a shadow root's host, 6 an element's shadow root (any
// mode), 7 the shadow-including parent (a shadow root's host), 8 the assigned slot (named slots)
uint32_t mrt_dom_link(void* host, uint32_t id, int32_t which);
int32_t mrt_dom_name(void* host, uint32_t id, const char** out, size_t* len);
// The document's mode: 0 no quirks, 1 limited quirks, 2 quirks.
int32_t mrt_dom_quirks_mode(void* host);
void mrt_dom_text_content(void* host, uint32_t id, const char** out, size_t* len);
void mrt_dom_set_text_content(void* host, uint32_t id, const char* s, size_t len);
uint32_t mrt_dom_create_element(void* host, const char* tag, size_t len);
uint32_t mrt_dom_create_element_ns(void* host, int32_t ns, const char* tag, size_t len);
uint32_t mrt_dom_create_text(void* host, const char* s, size_t len);
// 0 ok, 1 HierarchyRequestError, 2 NotFoundError
int32_t mrt_dom_insert(void* host, uint32_t parent, uint32_t child, uint32_t before);
// mrt_dom_insert's checks alone: 0, 1 (HierarchyRequestError) or 2 (NotFoundError).
int32_t mrt_dom_check_insert(void* host, uint32_t parent, uint32_t child, uint32_t before);
int32_t mrt_dom_remove(void* host, uint32_t parent, uint32_t child);
int32_t mrt_dom_get_attr(void* host, uint32_t id, const char* name, size_t name_len, const char** out, size_t* len);
void mrt_dom_set_attr(void* host, uint32_t id, const char* name, size_t name_len, const char* value, size_t value_len);
void mrt_dom_remove_attr(void* host, uint32_t id, const char* name, size_t name_len);
// The first element in tree order below `root` (a document or a shadow root) with this id.
uint32_t mrt_dom_get_element_by_id(void* host, uint32_t root, const char* s, size_t len);
// attachShadow with the SR_* flags below: the shadow root, or -1 for NotSupportedError.
int64_t mrt_dom_attach_shadow(void* host, uint32_t id, uint32_t flags);
// A shadow root's SR_* flags, -1 for another node.
int32_t mrt_dom_shadow_flags(void* host, uint32_t id);
// The slots of a shadow tree in tree order, read with mrt_dom_result; returns the count.
int32_t mrt_dom_slots(void* host, uint32_t root);
// A slot's assigned nodes, read with mrt_dom_result; returns the count.
int32_t mrt_dom_assigned_nodes(void* host, uint32_t slot);
// Nodes matching a selector below `root`, in tree order, at most `max` (0: all). Ids go to a list the
// caller reads with mrt_dom_result. Returns the count, or -1 for a selector that does not parse.
int32_t mrt_dom_query(void* host, uint32_t root, const char* sel, size_t len, uint32_t max);
uint32_t mrt_dom_result(void* host, uint32_t i);
// element.matches (closest 0: the element's id or 0) and element.closest (1: the nearest inclusive
// ancestor matching, or 0); -1 for a selector that does not parse.
int64_t mrt_dom_match(void* host, uint32_t id, const char* sel, size_t len, int32_t closest);
// The border box of an element in CSS px relative to the document (flushes style and layout);
// false when it has no frame.
int32_t mrt_dom_box(void* host, uint32_t id, double out[4]);
int32_t mrt_dom_client_scroll(void* host, uint32_t id, double out[4]);
// offsetParent (0: none) and offsetLeft, offsetTop, offsetWidth, offsetHeight (Element::GetOffsetRect)
uint32_t mrt_dom_offset_rect(void* host, uint32_t id, int32_t out[4]);
// What a ResizeObserver reads of each of n elements (ResizeObserver::CalculateBoxSize's three boxes and
// the contentRect): 11 numbers each into out, flushing their documents first if flush (src/host.rs
// mrt_dom_resize_boxes)
void mrt_dom_resize_boxes(void* host, const uint32_t* ids, size_t n, bool flush, double* out);
// IntersectionObserver's geometry for n targets of an observer of document owner with root (0: the
// implicit root): margins holds the rootMargin and the scrollMargin, each side a value and 1 for a
// percentage (16 numbers); 14 numbers each into out, in app units (src/host.rs mrt_dom_intersections)
void mrt_dom_intersections(void* host, uint32_t owner, uint32_t root, const double* margins, const uint32_t* ids, size_t n, double* out);
// elementFromPoint (multiple 0) and elementsFromPoint of root (a document or a shadow root) at (x, y) in
// CSS px: the number of elements, the first cap of them into out (src/host.rs mrt_dom_nodes_from_point)
size_t mrt_dom_nodes_from_point(void* host, uint32_t root, float x, float y, int multiple, uint32_t* out, size_t cap);
// innerText of an element: its rendered text, or its textContent when it is not rendered (src/host.rs
// mrt_dom_inner_text)
void mrt_dom_inner_text(void* host, uint32_t id, const char** out, size_t* len);
// The frames' layout (src/host.rs, DESIGN "The frames' layout (S5)"): a frame window's document
// opened (frame 0: a root window, laid out at width by height), changed (a frame's window got a new
// document), closed; a document's viewport (flushes its frame's parent; 0 when it has none); the
// refresh tick's state (op 0 resize pending, 1 the DOM's generation, 2 flush the frames' parents, 3
// take the documents resized into out).
void mrt_dom_window_opened(void* host, uint32_t doc, uint32_t frame, int32_t width, int32_t height);
void mrt_dom_window_document_changed(void* host, uint32_t old_doc, uint32_t doc);
void mrt_dom_window_closed(void* host, uint32_t doc);
int32_t mrt_dom_viewport(void* host, uint32_t doc, int32_t out[2]);
void mrt_dom_set_root_scroll(void* host, double x, double y);
double mrt_dom_refresh(void* host, int32_t op, uint32_t* out, size_t max);
// A node's document (nsINode::OwnerDoc), its document set (a node just made), and a subtree adopted
// into a document (src/host.rs; mrt_dom::Dom::node_document, set_node_document, adopt).
uint32_t mrt_dom_node_document(void* host, uint32_t id);
void mrt_dom_set_node_document(void* host, uint32_t id, uint32_t doc);
void mrt_dom_adopt(void* host, uint32_t id, uint32_t doc);
// A document's compatibility mode (0 no quirks, 1 limited, 2 quirks); a frame's initial
// about:blank; a frame's document parsed from its HTML with scripting on (mrt_dom::Dom).
int32_t mrt_dom_document_mode(void* host, uint32_t doc);
void mrt_dom_set_document_mode(void* host, uint32_t doc, int32_t mode);
uint32_t mrt_dom_create_blank_document(void* host);
uint32_t mrt_dom_parse_frame_document(void* host, const char* html, size_t len);
// The JS half of the bindings, to evaluate in every frame's window: 0 prelude.js, 1 bridge.js.
void mrt_js_set_window_script(MrtJs* js, int32_t which, const char* src, size_t len);
// getComputedStyle (src/host.rs mrt_dom_computed_style): op 0 the value of property `name` of element
// `id` with pseudo-element `pseudo` (flushes style and layout), op 1 the length, op 2 the name at index
// `name`.
int32_t mrt_dom_computed_style(void* host, uint32_t id, int32_t op, const char* pseudo, size_t pseudo_len, const char* name,
                               size_t name_len, const char** out, size_t* out_len);
void mrt_dom_link_sheet(void* host, uint32_t id, const char* url, size_t url_len, const char* css, size_t css_len, int32_t loaded);
void mrt_console(void* host, const char* s, size_t len);
// createComment (kind 3, with its text) and createDocumentFragment (kind 5).
uint32_t mrt_dom_create_node(void* host, int32_t kind, const char* s, size_t len);
uint32_t mrt_dom_clone(void* host, uint32_t id, int32_t deep);
// innerHTML: the markup parsed as body content into a new fragment; returns the fragment.
// markup parsed into a new fragment in the context of element `context` (0: a <body>)
uint32_t mrt_dom_parse_fragment(void* host, const char* s, size_t len, uint32_t context);
// The attribute names of an element, separated by newlines.
void mrt_dom_attr_names(void* host, uint32_t id, const char** out, size_t* len);
// createHTMLDocument: a new document (with a title element if has_title).
uint32_t mrt_dom_create_html_document(void* host, const char* title, size_t len, int32_t has_title);
// DOMParser's parseFromString for text/html: a new document (scripting off, no declarative shadow
// roots).
uint32_t mrt_dom_parse_document(void* host, const char* s, size_t len);
// Constructed style sheets: a new one with this text (its id), a new text for one, and the sheets a
// document or shadow root adopted (adoptedStyleSheets).
uint32_t mrt_dom_new_sheet(void* host, const char* s, size_t len);
void mrt_dom_set_sheet(void* host, uint32_t id, const char* s, size_t len);
void mrt_dom_set_adopted(void* host, uint32_t node, const uint32_t* ids, size_t n);
// The CSSOM of a style sheet (src/host.rs mrt_cssom_op): element `id`'s when `owner` is 1, else
// constructed sheet `id`; the result string in *out.
void mrt_cssom_op(void* host, int32_t op, int32_t owner, uint32_t id, const char* s, size_t len, double n,
                  const char** out, size_t* out_len);
// A <template>'s contents fragment, 0 for any other node.
uint32_t mrt_dom_template_contents(void* host, uint32_t id);
uint64_t mrt_dom_take_script_position(void* host, uint32_t id);
// Whether a name is a valid custom element name.
int32_t mrt_dom_is_custom_element_name(const char* s, size_t len);
// An element's custom element state (CE_* below), -1 for a node that is not an element.
int32_t mrt_dom_ce_state(void* host, uint32_t id);
void mrt_dom_set_ce_state(void* host, uint32_t id, int32_t state);
// element.style over Stylo's declaration block (src/host.rs mrt_style_op): op 0 get, 1 priority,
// 2 set (the new attribute text in *out, returns 1 when it changed), 3 remove (the same), 4 cssText,
// 5 length, 6 item (the index in `value`), 7 whether `name` is a CSS property ("1" or "").
int32_t mrt_style_op(void* host, int32_t op, const char* css, size_t css_len, const char* name, size_t name_len,
                     const char* value, size_t value_len, int32_t important, const char** out, size_t* out_len);
// Stage 6's bridge to Firefox: sends one request line (JSON) and blocks for the reply line. Returns
// 0 when the page has no bridge (jscount and the tests).
int32_t mrt_bridge(void* host, const char* req, size_t len, const char** out, size_t* out_len);
// Dedicated and shared workers (DESIGN, "Workers: the build plan"; GAPS B85): a worker's context and
// global, made on its own thread (worker.rs), a SharedWorkerGlobalScope when `shared`; the page's
// requests to its workers (__mrtWorker: JSON in, JSON out); what a worker hands its parent at the end
// of a task (__mrtWorkerOut).
MrtJs* mrt_js_new_worker(void* host, const MrtGlobalOptions* worker, int shared);
int32_t mrt_worker_new(void* host, const char* config, size_t config_len, const char* origin, size_t origin_len,
                       int32_t secure, int32_t isolated, double at, int32_t shared);
void mrt_worker_post(void* host, double id, const char* data, size_t len, double at);
int32_t mrt_worker_next(void* host, double* at);
void mrt_worker_run(void* host, double id, const char** out, size_t* out_len);
void mrt_worker_terminate(void* host, double id);
void mrt_worker_out(void* host, const char* s, size_t len);

#ifdef __cplusplus
}
#endif
