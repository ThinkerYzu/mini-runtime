// The C++ layer over the Firefox tree's SpiderMonkey: the context and global, the node reflectors
// with their wrapper cache, the DOM natives, and the memory report. Every DOM read or change goes
// to Rust through the mrt_dom_* callbacks (mrt_js.h); nothing of the DOM lives here.

#include <jsapi.h>
#include <jsfriendapi.h>
#include <js/Array.h>
#include <js/ArrayBuffer.h>
#include <js/CallAndConstruct.h>
#include <js/ForOfIterator.h>
#include <js/CharacterEncoding.h>
#include <js/CompilationAndEvaluation.h>
#include <js/Debug.h>
#include <js/Conversions.h>
#include <js/ErrorReport.h>
#include <js/Exception.h>
#include <js/friend/ErrorMessages.h>
#include <js/GCAPI.h>
#include <js/Initialization.h>
#include <js/MemoryMetrics.h>
#include <js/Modules.h>
#include <js/Principals.h>
#include <js/RealmIterators.h>
#include <js/Wrapper.h>
#include <js/Zone.h>
#include <js/Object.h>
#include <js/Promise.h>
#include <js/PropertyAndElement.h>
#include <js/PropertySpec.h>
#include <js/Proxy.h>
#include <js/SourceText.h>
#include <js/StableStringChars.h>
#include <js/String.h>
#include <js/TracingAPI.h>
#include <js/experimental/TypedData.h>

#include <malloc.h>
#include <algorithm>
#include <chrono>
#include <cmath>
#include <deque>
#include <cstdio>
#include <thread>
#include <cstring>
#include <ctime>
#include <pthread.h>
#include <memory>
#include <optional>
#include <map>
#include <mutex>
#include <string>
#include <unordered_map>
#include <vector>
#include <unordered_set>

#include <brotli/decode.h>
#include <brotli/encode.h>
#include "zlib.h"

#include "mrt_js.h"
#include "html_interfaces.h"

// Prototype slots of the global, after the ones SpiderMonkey reserves (JSCLASS_GLOBAL_SLOT_COUNT),
// one per kind of reflector (Firefox has one prototype per WebIDL
// interface; the HTML element interfaces share one here, see DESIGN "Bindings").
enum ProtoSlot {
  SLOT_EVENT_TARGET = JSCLASS_GLOBAL_SLOT_COUNT,
  SLOT_WINDOW,
  SLOT_NODE,
  SLOT_ELEMENT,
  SLOT_HTML_ELEMENT,
  SLOT_SVG_ELEMENT,
  SLOT_CHARACTER_DATA,
  SLOT_TEXT,
  SLOT_COMMENT,
  SLOT_DOCUMENT,
  SLOT_DOCUMENT_TYPE,
  SLOT_DOCUMENT_FRAGMENT,
  SLOT_SHADOW_ROOT,
  SLOT_CUSTOM_ELEMENT_REGISTRY,
  // interfaces with no nodes in an HTML document, there for scripts that patch their prototypes
  SLOT_CDATA_SECTION,
  SLOT_PROCESSING_INSTRUCTION,
  SLOT_MUTATION_OBSERVER,
  SLOT_MUTATION_RECORD,
  // a worker's global (DESIGN, "Workers: the build plan"): WorkerGlobalScope's prototype, and its own
  // interface's, DedicatedWorkerGlobalScope or SharedWorkerGlobalScope
  SLOT_WORKER_GLOBAL_SCOPE,
  SLOT_WORKER_KIND_SCOPE,
  // one per HTML element interface below HTMLElement (html_interfaces.h)
  SLOT_HTML_INTERFACES,
  SLOT_COUNT = SLOT_HTML_INTERFACES + HTML_INTERFACE_COUNT
};

// Node kinds as mrt_dom::Kind numbers them.
enum NodeKind { KIND_DOCUMENT, KIND_ELEMENT, KIND_TEXT, KIND_COMMENT, KIND_DOCTYPE, KIND_FRAGMENT, KIND_SHADOW_ROOT };
// mrt_dom_link's `which`
enum NodeLink { LINK_PARENT, LINK_FIRST, LINK_LAST, LINK_NEXT, LINK_PREV, LINK_HOST, LINK_SHADOW_ROOT,
                LINK_SHADOW_INCLUDING_PARENT, LINK_ASSIGNED_SLOT };
// A shadow root's options, as mrt_dom's SR_* constants.
enum ShadowRootFlags { SR_CLOSED = 1, SR_CLONABLE = 2, SR_SERIALIZABLE = 4, SR_DELEGATES_FOCUS = 8, SR_MANUAL_SLOTS = 16,
                       SR_DECLARATIVE = 32 };
static const int32_t NS_HTML = 3;
static const int32_t NS_SVG = 6;

// ---- custom elements: the data (dom/base/CustomElementRegistry.h) ----

// CustomElementData::State, kept in the arena node's flags (mrt_dom::CE_*).
enum CustomElementState { CE_UNCUSTOMIZED = 0, CE_UNDEFINED = 1, CE_FAILED = 2, CE_PRECUSTOMIZED = 3, CE_CUSTOM = 4 };

enum class ElementCallbackType { eConnected, eDisconnected, eAdopted, eAttributeChanged };

// LifecycleCallbackArgs: the attribute's name, old and new values (nullopt is null) for
// attributeChangedCallback; the namespace is always null (no namespaced attributes from script).
struct LifecycleCallbackArgs {
  std::string mName;
  std::optional<std::string> mOldValue, mNewValue;
};

struct CustomElementDefinition {
  std::string mType;       // the name it was defined with
  std::string mLocalName;  // the same: customized built-in elements are not supported
  JS::Heap<JSObject*> mConstructor;
  // LifecycleCallbacks; null when the prototype has none
  JS::Heap<JSObject*> mConnectedCallback, mDisconnectedCallback, mAdoptedCallback, mAttributeChangedCallback;
  std::vector<std::string> mObservedAttributes;
  bool mFormAssociated = false, mDisableInternals = false, mDisableShadow = false;
  // Elements being upgraded, innermost last; ALREADY_CONSTRUCTED_MARKER once its constructor ran.
  std::vector<uint32_t> mConstructionStack;

  bool IsInObservedAttributeList(const std::string& aName) const {
    for (const std::string& a : mObservedAttributes) {
      if (a == aName) return true;
    }
    return false;
  }
};
static const uint32_t ALREADY_CONSTRUCTED_MARKER = UINT32_MAX;

// One entry of an element's reaction queue: an upgrade, or a lifecycle callback with its arguments.
struct CustomElementReaction {
  CustomElementDefinition* mDefinition;  // the upgrade's definition; the callback's owner
  bool mIsUpgrade;
  ElementCallbackType mType;
  LifecycleCallbackArgs mArgs;
};

// The document's CustomElementRegistry and the DocGroup's CustomElementReactionsStack (one page,
// one of each). Elements are node ids; the per-element CustomElementData lives in the two maps.
struct CustomElements {
  std::vector<std::unique_ptr<CustomElementDefinition>> mDefinitions;  // in definition order
  std::unordered_map<std::string, CustomElementDefinition*> mCustomDefinitions;
  std::unordered_map<std::string, JS::Heap<JSObject*>> mWhenDefinedPromiseMap;
  bool mIsCustomDefinitionRunning = false;
  // CustomElementData::mCustomElementDefinition and mReactionQueue
  std::unordered_map<uint32_t, CustomElementDefinition*> mElementDefinition;
  std::unordered_map<uint32_t, std::vector<CustomElementReaction>> mReactionQueue;
  // CustomElementReactionsStack
  std::vector<std::vector<uint32_t>> mReactionsStack;
  std::vector<uint32_t> mBackupQueue;
  uint32_t mRecursionDepth = 0;
  bool mIsElementQueuePushedForCurrentRecursionDepth = false;
  bool mIsBackupQueueProcessing = false;
  // Set while a bridge.js method made of several DOM changes runs (__mrtCEReactions): the natives it
  // calls do not open reaction scopes of their own, as the steps of one Gecko method do not.
  bool mInCompoundMethod = false;
};

// ---- module scripts: the data (js/loader/LoadedScript.h, ModuleLoaderBase.h); the loader is below ----

// ScriptFetchInfo: what a module record's private points at; here only the base URL (the URL the
// module was fetched from, after redirects), against which its imports resolve.
struct ScriptFetchInfo {
  std::string mBaseURL;
};

// ModuleScript (js/loader/LoadedScript.h): a compiled module, or the error compiling it gave.
struct ModuleScript {
  ScriptFetchInfo mFetchInfo;
  JS::Heap<JSObject*> mModuleRecord;
  JS::Heap<JS::Value> mParseError;
  JS::Heap<JS::Value> mErrorToRethrow;
  bool HasParseError() const { return !mParseError.unbarrieredGet().isUndefined(); }
  bool HasErrorToRethrow() const { return !mErrorToRethrow.unbarrieredGet().isUndefined(); }
};

// The module map (mFetchedModules), keyed by type and URL as Gecko's ModuleMapKey. A null entry is a
// fetch that failed. Inline module scripts are not in it; their scripts are kept in `mInline` for
// as long as the page lives, as their records may be imported from (a cycle) or hold import.meta.
struct ModuleLoader {
  std::unordered_map<std::string, std::unique_ptr<ModuleScript>> mFetchedModules;
  std::vector<std::unique_ptr<ModuleScript>> mInline;
  // for the report: module fetches made, and those that failed
  size_t mFetches = 0, mFetchFailures = 0;
};

// ---- mutation observers: the data (dom/base/nsDOMMutationObserver.h) ----
//
// Translated from Gecko with its names. A record holds node ids, not JS objects: arena nodes are
// never freed (GAPS B10), so an id stays valid, and the reflectors are made when the records are
// delivered. Attributes have no namespace here (setAttributeNS keys by the qualified name, B15).

struct nsDOMMutationObserver;

// nsDOMMutationRecord; the type is the record's `type` string. A void attribute name or old value
// is nullopt (null in script).
struct nsDOMMutationRecord {
  const char* mType = nullptr;  // "attributes", "characterData" or "childList"
  uint32_t mTarget = 0;
  std::optional<std::string> mAttrName;
  std::optional<std::string> mPrevValue;
  std::vector<uint32_t> mAddedNodes, mRemovedNodes;
  uint32_t mPreviousSibling = 0, mNextSibling = 0;
};

// nsMutationReceiverBase and nsMutationReceiver: one registration of an observer on a node, or a
// transient copy of one (`mParent`) put on a node removed from the observed subtree until the
// observer's next delivery. A transient receiver's options are its parent's.
struct nsMutationReceiver {
  nsDOMMutationObserver* mObserver = nullptr;  // null for a transient receiver
  nsMutationReceiver* mParent = nullptr;       // cleared after the microtask
  uint32_t mTarget = 0;                        // observe()'s target; 0 for a transient receiver
  uint32_t mRegisterTarget = 0;                // the node it is registered on
  std::vector<nsMutationReceiver*> mTransientReceivers;  // its clones (owned by the observer)
  std::vector<std::string> mAttributeFilter;
  bool mSubtree = false, mChildList = false, mCharacterData = false, mCharacterDataOldValue = false,
       mAttributes = false, mAllAttributes = false, mAttributeOldValue = false;

  nsDOMMutationObserver* Observer() { return mParent ? mParent->Observer() : mObserver; }
  uint32_t Target() { return mParent ? mParent->Target() : mTarget; }
  uint32_t RegisterTarget() { return mRegisterTarget; }
  nsMutationReceiver* GetParent() { return mParent; }
  bool Subtree() { return mParent ? mParent->Subtree() : mSubtree; }
  bool ChildList() { return mParent ? mParent->ChildList() : mChildList; }
  bool CharacterData() { return mParent ? mParent->CharacterData() : mCharacterData; }
  bool CharacterDataOldValue() { return mParent ? mParent->CharacterDataOldValue() : mCharacterDataOldValue; }
  bool Attributes() { return mParent ? mParent->Attributes() : mAttributes; }
  bool AllAttributes() { return mParent ? mParent->AllAttributes() : mAllAttributes; }
  bool AttributeOldValue() { return mParent ? mParent->AttributeOldValue() : mAttributeOldValue; }
  std::vector<std::string>& AttributeFilter() { return mParent ? mParent->AttributeFilter() : mAttributeFilter; }
};

// nsDOMMutationObserver. Its reflector holds it (reserved slot) and frees it when finalized; the
// reflector is kept alive while the observer observes a node or has records to deliver (Gecko:
// nsINode::BindObject and the scheduled list hold it).
struct nsDOMMutationObserver {
  JS::Heap<JSObject*> mReflector;
  JS::Heap<JSObject*> mCallback;
  std::vector<std::unique_ptr<nsMutationReceiver>> mReceivers;
  // the transient receivers by the node they are registered on
  std::vector<std::pair<uint32_t, std::vector<std::unique_ptr<nsMutationReceiver>>>> mTransientReceivers;
  std::vector<std::shared_ptr<nsDOMMutationRecord>> mCurrentMutations;  // one per mutation level
  std::vector<std::shared_ptr<nsDOMMutationRecord>> mPendingMutations;
  bool mWaitingForRun = false;
  uint64_t mId = 0;
  // whether something but its reflector holds it: it observes, or has records or transient
  // receivers until its next delivery
  bool Held() const {
    return !mReceivers.empty() || mWaitingForRun || !mPendingMutations.empty() || !mTransientReceivers.empty();
  }
};

class nsAutoMutationBatch;

// Gecko's statics (nsDOMMutationObserver's and nsAutoMutationBatch's), per context, and the nodes'
// observer lists (nsINode::GetMutationObservers).
struct MutationObservers {
  std::vector<std::unique_ptr<nsDOMMutationObserver>> mObservers;
  std::unordered_map<uint32_t, std::vector<nsMutationReceiver*>> mNodeObservers;
  // Document::MayHaveDOMMutationObservers: set by the first observe(), never cleared
  bool mMayHaveDOMMutationObservers = false;
  std::optional<std::vector<nsDOMMutationObserver*>> sScheduledMutationObservers;
  uint32_t sMutationLevel = 0;
  uint64_t sCount = 0;
  std::vector<std::vector<nsDOMMutationObserver*>> sCurrentlyHandlingObservers;
  nsAutoMutationBatch* sCurrentBatch = nullptr;
  // DocGroup::sPendingDocGroups: whether a microtask waits for the signal slot list
  bool sPendingSignalSlots = false;
};

// ---- sessionStorage (dom/storage/SessionStorage.cpp, SessionStorageCache.cpp) ----
//
// sessionStorage is the tab's, so it is page state and lives here (SPEC decision 14), where
// localStorage, the origin's, is forwarded to Firefox. One SessionStorageCache per origin, as
// SessionStorageManager keeps them. Strings are UTF-16, as nsString, so that the quota counts what
// Gecko counts. Not translated: the write optimizer and the actor (the cache's copy in the parent
// process, for session restore; the runtime's save does that, DESIGN "Page state outside the
// arena"), the storage event (it goes to the other same-origin documents of the tab: there are none
// without frames). Keys are in insertion order; Gecko's nsTHashMap order is its hash table's (GAPS).
class SessionStorageCache {
 public:
  struct DataSet {
    // dom.storage.default_quota (5 * 1024 KB), LocalStorageManager::GetOriginQuota
    static constexpr int64_t kOriginQuota = 5 * 1024 * 1024;
    bool ProcessUsageDelta(int64_t aDelta) {
      // Check limit per this origin
      int64_t newOriginUsage = mOriginQuotaUsage + aDelta;
      if (aDelta > 0 && newOriginUsage > kOriginQuota) {
        return false;
      }
      // Update size in our data set
      mOriginQuotaUsage = newOriginUsage;
      return true;
    }
    int64_t mOriginQuotaUsage = 0;
    // mKeys: the items, and an index into them by key
    std::vector<std::pair<std::u16string, std::u16string>> mKeys;
    std::unordered_map<std::u16string, size_t> mIndex;
  };

  uint32_t Length() const { return mDataSet.mKeys.size(); }
  // aResult is null (false) past the end
  bool Key(uint32_t aIndex, std::u16string& aResult) const {
    if (aIndex >= mDataSet.mKeys.size()) return false;
    aResult = mDataSet.mKeys[aIndex].first;
    return true;
  }
  bool GetItem(const std::u16string& aKey, std::u16string& aResult) const {
    auto it = mDataSet.mIndex.find(aKey);
    if (it == mDataSet.mIndex.end()) return false;
    aResult = mDataSet.mKeys[it->second].second;
    return true;
  }
  // NS_ERROR_DOM_QUOTA_EXCEEDED_ERR is kQuotaExceeded, NS_SUCCESS_DOM_NO_OPERATION kNoOperation
  enum Result { kOk, kNoOperation, kQuotaExceeded };
  Result SetItem(const std::u16string& aKey, const std::u16string& aValue) {
    int64_t delta = 0;
    auto it = mDataSet.mIndex.find(aKey);
    const std::u16string* oldValue = it == mDataSet.mIndex.end() ? nullptr : &mDataSet.mKeys[it->second].second;
    if (!oldValue) {
      // We only consider key size if the key doesn't exist before.
      delta = static_cast<int64_t>(aKey.size());
    }
    delta += static_cast<int64_t>(aValue.size()) - static_cast<int64_t>(oldValue ? oldValue->size() : 0);
    if (oldValue && *oldValue == aValue) {
      return kNoOperation;
    }
    if (!mDataSet.ProcessUsageDelta(delta)) {
      return kQuotaExceeded;
    }
    if (oldValue) {
      mDataSet.mKeys[it->second].second = aValue;
    } else {
      mDataSet.mIndex.emplace(aKey, mDataSet.mKeys.size());
      mDataSet.mKeys.emplace_back(aKey, aValue);
    }
    return kOk;
  }
  Result RemoveItem(const std::u16string& aKey) {
    auto it = mDataSet.mIndex.find(aKey);
    if (it == mDataSet.mIndex.end()) {
      return kNoOperation;
    }
    size_t i = it->second;
    // Recalculate the cached data size
    mDataSet.ProcessUsageDelta(-(static_cast<int64_t>(mDataSet.mKeys[i].second.size()) + static_cast<int64_t>(aKey.size())));
    mDataSet.mIndex.erase(it);
    mDataSet.mKeys.erase(mDataSet.mKeys.begin() + i);
    for (size_t j = i; j < mDataSet.mKeys.size(); j++) mDataSet.mIndex[mDataSet.mKeys[j].first] = j;
    return kOk;
  }
  void Clear() {
    mDataSet.ProcessUsageDelta(-mDataSet.mOriginQuotaUsage);
    mDataSet.mKeys.clear();
    mDataSet.mIndex.clear();
  }
  int64_t GetOriginQuotaUsage() const { return mDataSet.mOriginQuotaUsage; }
  // Heap bytes, for the memory report: the strings' buffers and the two tables
  size_t SizeOfExcludingThis() const {
    size_t n = mDataSet.mKeys.capacity() * sizeof(mDataSet.mKeys[0]);
    for (auto& [k, v] : mDataSet.mKeys) n += (2 * k.capacity() + v.capacity()) * sizeof(char16_t);  // the key twice: in the index too
    return n + mDataSet.mIndex.bucket_count() * sizeof(void*) + mDataSet.mIndex.size() * 32;
  }

 private:
  DataSet mDataSet;
};

struct MrtJs {
  void* host = nullptr;
  JSContext* cx = nullptr;
  std::unique_ptr<JS::PersistentRootedObject> global;
  // Globals made by mrt_js_new_frame_global (the zone and compartment rules for frames; frames
  // themselves are not run yet).
  std::vector<std::unique_ptr<JS::PersistentRootedObject>> frames;
  // Rejected promises nobody handled yet, as Gecko keeps them (CycleCollectedJSContext): `uncaught`
  // for the console report (mUncaughtRejections), `aboutToBeNotified` for the unhandledrejection
  // event (mAboutToBeNotifiedRejectedPromises), and the ids of those the event was fired for and
  // that are still unhandled (mPendingUnhandledRejections). A handled promise leaves a null slot.
  std::vector<std::unique_ptr<JS::PersistentRootedObject>> uncaught, aboutToBeNotified, rejectionHandled;
  std::vector<uint64_t> pendingUnhandled;
  // Batches waiting for their task (__mrtRejectionTask): the unhandledrejection events of one
  // checkpoint (Gecko's NotifyUnhandledRejections runnable) and rejectionhandled events
  // (AsyncEventDispatcher).
  std::deque<std::vector<std::unique_ptr<JS::PersistentRootedObject>>> notifyBatches, handledBatches;
  // The wrapper cache: node id to reflector, like nsWrapperCache::mWrapper in each Gecko node.
  // Weak unless `preserved` (see DESIGN "Reflectors and the wrapper cache").
  std::vector<JS::Heap<JSObject*>> cache;
  std::vector<uint8_t> preserved;
  size_t errors = 0;
  // HTML's "performing a microtask checkpoint" flag: set while MicrotaskCheckpoint runs the jobs, so
  // that a listener called from one of them (at callback depth 0) does not start another.
  bool performingCheckpoint = false;
  // Page code on the stack that the runtime called (CycleCollectedJSContext::mMicroTaskLevel): a
  // source Rust evaluates other than the runtime's own `driver` and `worker` calls, and each callback
  // the glue calls between __mrtEnterCallback and __mrtLeaveCallback. Shared by the page's windows.
  uint32_t callbackDepth = 0;
  CustomElements ce;
  // Slots whose assigned nodes changed, waiting for their slotchange event (SlotChangeMicroTask).
  std::vector<uint32_t> signalSlots;
  ModuleLoader modules;
  MutationObservers mo;
  // sessionStorage by origin (SessionStorageManager's caches)
  std::map<std::string, SessionStorageCache> sessionStorage;
  // The windows of same-origin frames (DESIGN, "Same-origin frames: the build plan"): each frame
  // element's document and global, in the page's zone and compartment (SPEC decision 15). The page's
  // own window (document 1, `global`) is not in the list. A closed window stays, so its global stays
  // the one its nodes' reflectors were made in.
  struct FrameWindow {
    uint32_t doc;
    // the frame element, 0 for a root window (its embedder in another runtime)
    uint32_t frame;
    std::unique_ptr<JS::PersistentRootedObject> global;
    bool closed;
  };
  std::vector<FrameWindow> windows;
  // The JS half of the bindings, evaluated in every window (Page::with_global, Page::load_bridge).
  std::string preludeSource, bridgeSource;
  // a dedicated worker's context (mrt_js_new_worker), not a page's
  bool worker = false;
  // The nodes the document's parser put into the tree since its last stop (mrt_js_parser_inserted),
  // for the reactions that wait for the stop (mrt_js_parser_stopped).
  std::vector<uint32_t> parserInserted;
};

// One context per thread, so the hooks that get no closure find it here.
static thread_local MrtJs* tJs = nullptr;

// The global of document `doc`'s window: the page's for document 1, a frame's for its document
// (closed or not: its nodes keep their reflectors' global), null for a document without a window
// (createHTMLDocument, DOMParser).
static JSObject* WindowGlobalForDoc(uint32_t doc) {
  if (doc == 1) return *tJs->global;
  for (auto& w : tJs->windows)
    if (w.doc == doc) return *w.global;
  return nullptr;
}
// The frame window whose global is `global`, or null (the page's, or not a window).
static MrtJs::FrameWindow* FrameWindowOf(JSObject* global) {
  for (auto& w : tJs->windows)
    if (*w.global == global) return &w;
  return nullptr;
}
static bool ViewOfWindow(JSContext* cx, JSObject* aTarget, JS::MutableHandleValue rval);
static bool SameOriginGlobals(JSObject* aFirst, JSObject* aSecond);

// JS::DefaultGlobalClassOps, with a resolve hook that also makes the HTML element interfaces on first use.
static bool Global_resolve(JSContext* cx, JS::HandleObject obj, JS::HandleId id, bool* resolvedp);
static bool Global_mayResolve(const JSAtomState& names, jsid id, JSObject* maybeObj);
static const JSClassOps kGlobalClassOps = {nullptr, nullptr, nullptr, JS_NewEnumerateStandardClasses, Global_resolve,
                                           Global_mayResolve, nullptr, nullptr, nullptr, JS_GlobalObjectTraceHook};
static const JSClass kGlobalClass = {"Window", JSCLASS_GLOBAL_FLAGS_WITH_SLOTS(SLOT_COUNT - JSCLASS_GLOBAL_SLOT_COUNT), &kGlobalClassOps};

// A property added to a reflector by script (an expando) would be lost if the reflector were
// collected and made again, so it preserves the wrapper, as Gecko's generated addProperty hooks do
// (CGAddPropertyHook calls PreserveWrapper).
static bool Reflector_addProperty(JSContext* cx, JS::HandleObject obj, JS::HandleId id, JS::HandleValue v) {
  int32_t node = JS::GetReservedSlot(obj, 0).toInt32();
  if (tJs && size_t(node) < tJs->preserved.size()) {
    tJs->preserved[node] = 1;
  }
  return true;
}

static const JSClassOps kReflectorOps = {Reflector_addProperty, nullptr, nullptr, nullptr, nullptr,
                                         nullptr, nullptr, nullptr, nullptr, nullptr};
static const JSClass kReflectorClass = {"Node", JSCLASS_HAS_RESERVED_SLOTS(1), &kReflectorOps};

// ---- GC hooks of the wrapper cache ----

static void TracePreserved(JSTracer* trc, void* data) {
  MrtJs* js = static_cast<MrtJs*>(data);
  for (size_t i = 0; i < js->cache.size(); i++) {
    if (js->preserved[i] && js->cache[i].unbarrieredGet()) {
      JS::TraceEdge(trc, &js->cache[i], "preserved wrapper");
    }
  }
  // the registry's definitions and promises (CustomElementRegistry::TraceDefinitions)
  for (auto& def : js->ce.mDefinitions) {
    JS::TraceEdge(trc, &def->mConstructor, "custom element constructor");
    for (JS::Heap<JSObject*>* cb : {&def->mConnectedCallback, &def->mDisconnectedCallback, &def->mAdoptedCallback,
                                    &def->mAttributeChangedCallback}) {
      if (cb->unbarrieredGet()) JS::TraceEdge(trc, cb, "custom element callback");
    }
  }
  for (auto& [name, promise] : js->ce.mWhenDefinedPromiseMap) {
    JS::TraceEdge(trc, &promise, "custom element whenDefined promise");
  }
  // the module map and the inline module scripts (ModuleScript is traced by the cycle collector)
  auto traceModule = [trc](ModuleScript* ms) {
    if (!ms) return;
    if (ms->mModuleRecord.unbarrieredGet()) JS::TraceEdge(trc, &ms->mModuleRecord, "module record");
    JS::TraceEdge(trc, &ms->mParseError, "module parse error");
    JS::TraceEdge(trc, &ms->mErrorToRethrow, "module error to rethrow");
  };
  for (auto& [key, ms] : js->modules.mFetchedModules) traceModule(ms.get());
  for (auto& ms : js->modules.mInline) traceModule(ms.get());
  // the mutation observers that observe or have records to deliver (Gecko: the observed nodes and
  // the scheduled list hold them)
  for (auto& o : js->mo.mObservers) {
    if (o->Held()) JS::TraceEdge(trc, &o->mReflector, "MutationObserver");
  }
}

static void SweepCache(JSTracer* trc, void* data) {
  MrtJs* js = static_cast<MrtJs*>(data);
  for (size_t i = 0; i < js->cache.size(); i++) {
    if (!js->preserved[i] && js->cache[i].unbarrieredGet()) {
      JS_UpdateWeakPointerAfterGC(trc, &js->cache[i]);
    }
  }
  // an observer held by nothing but its reflector goes with it
  auto& observers = js->mo.mObservers;
  for (size_t i = 0; i < observers.size();) {
    nsDOMMutationObserver* o = observers[i].get();
    if (!o->Held() && !JS_UpdateWeakPointerAfterGC(trc, &o->mReflector)) {
      observers.erase(observers.begin() + i);
    } else {
      i++;
    }
  }
}

// ---- strings ----

static bool NewString(JSContext* cx, const char* s, size_t len, JS::MutableHandleValue rval) {
  JSString* str = JS_NewStringCopyUTF8N(cx, JS::UTF8Chars(s, len));
  if (!str) return false;
  rval.setString(str);
  return true;
}

// The argument as UTF-8, converted as the WebIDL DOMString conversion does (ToString).
static bool ArgUTF8(JSContext* cx, JS::HandleValue v, std::string& out) {
  JS::RootedString str(cx, JS::ToString(cx, v));
  if (!str) return false;
  JS::UniqueChars chars = JS_EncodeStringToUTF8(cx, str);
  if (!chars) return false;
  out.assign(chars.get());
  return true;
}

// ---- reflectors ----

// The prototype slot of an element's interface, by its tag as Gecko's NS_NewHTMLElement picks the
// class (nsHTMLTagList.inc): HTMLElement for the tags without one of their own and for custom
// element names, HTMLUnknownElement for any other unknown tag. SVG namespace reflectors inherit
// SVGElement (SVGElement.webidl), enabling namespace checks during Vue mounting. SVG-specific
// interfaces and the existing MathML fallback remain unimplemented (GAPS B18).
static int ElementProtoSlot(uint32_t id) {
  static const std::unordered_map<std::string, int> tags = [] {
    std::unordered_map<std::string, int> m;
    for (const auto& t : kHTMLTags) m[t.tag] = t.interface;
    return m;
  }();
  static const int unknown = [] {
    for (int i = 0; i < HTML_INTERFACE_COUNT; i++) {
      if (!strcmp(kHTMLInterfaceNames[i], "HTMLUnknownElement")) return i;
    }
    return -1;
  }();
  const char* s;
  size_t len;
  const int32_t ns = mrt_dom_name(tJs->host, id, &s, &len);
  if (ns == NS_SVG) return SLOT_SVG_ELEMENT;
  if (ns != NS_HTML) return SLOT_HTML_ELEMENT;
  auto it = tags.find(std::string(s, len));
  int iface = it != tags.end() ? it->second : mrt_dom_is_custom_element_name(s, len) ? -1 : unknown;
  return iface < 0 ? SLOT_HTML_ELEMENT : SLOT_HTML_INTERFACES + iface;
}

static int ProtoSlotOf(uint32_t id) {
  switch (mrt_dom_kind(tJs->host, id)) {
    case KIND_DOCUMENT: return SLOT_DOCUMENT;
    case KIND_ELEMENT: return ElementProtoSlot(id);
    case KIND_TEXT: return SLOT_TEXT;
    case KIND_COMMENT: return SLOT_COMMENT;
    case KIND_DOCTYPE: return SLOT_DOCUMENT_TYPE;
    case KIND_SHADOW_ROOT: return SLOT_SHADOW_ROOT;
    default: return SLOT_DOCUMENT_FRAGMENT;
  }
}

static bool EnsureHTMLInterface(JSContext* cx, JS::HandleObject global, int i);

// The reflector of node `id` (null for 0): the cached one, or a new one put in the cache.
static bool Wrap(JSContext* cx, uint32_t id, JS::MutableHandleValue rval) {
  if (id == 0) {
    rval.setNull();
    return true;
  }
  MrtJs* js = tJs;
  if (id < js->cache.size() && js->cache[id]) {
    rval.setObject(*js->cache[id].get());
    return js->windows.empty() || JS_WrapValue(cx, rval);
  }
  // the reflector is made in the global of the node's document's window, with its prototypes (Gecko's
  // GetParentObject); a node of a document without a window, in the current one
  JS::RootedObject global(cx, JS::CurrentGlobalOrNull(cx));
  if (!js->windows.empty()) {
    if (JSObject* g = WindowGlobalForDoc(mrt_dom_node_document(js->host, id))) global = g;
  }
  mozilla::Maybe<JSAutoRealm> realm;
  if (global != JS::CurrentGlobalOrNull(cx)) realm.emplace(cx, global);
  int slot = ProtoSlotOf(id);
  if (slot >= SLOT_HTML_INTERFACES && !EnsureHTMLInterface(cx, global, slot - SLOT_HTML_INTERFACES)) return false;
  JS::RootedObject proto(cx, &JS::GetReservedSlot(global, slot).toObject());
  JS::RootedObject obj(cx, JS_NewObjectWithGivenProto(cx, &kReflectorClass, proto));
  if (!obj) return false;
  JS_SetReservedSlot(obj, 0, JS::Int32Value(int32_t(id)));
  if (id >= js->cache.size()) {
    size_t n = std::max<size_t>(id + 1, js->cache.size() * 2);
    js->cache.resize(n);
    js->preserved.resize(n);
  }
  js->cache[id] = obj;
  realm.reset();
  rval.setObject(*obj);
  return JS_WrapValue(cx, rval);
}

// The node id of a value that is a reflector, 0 otherwise.
static uint32_t NodeOf(JS::HandleValue v) {
  if (!v.isObject()) return 0;
  JSObject* obj = &v.toObject();
  if (JS::GetClass(obj) != &kReflectorClass) return 0;
  return uint32_t(JS::GetReservedSlot(obj, 0).toInt32());
}

// The node `this` stands for; throws a TypeError when `this` is not a node (a brand check).
static bool ThisNode(JSContext* cx, const JS::CallArgs& args, uint32_t* id) {
  *id = NodeOf(args.thisv());
  if (!*id) {
    JS_ReportErrorUTF8(cx, "TypeError: 'this' is not a Node");
    return false;
  }
  return true;
}

static bool ArgNode(JSContext* cx, const JS::CallArgs& args, unsigned i, uint32_t* id, bool nullable = false) {
  if (nullable && args.get(i).isNullOrUndefined()) {
    *id = 0;
    return true;
  }
  *id = NodeOf(args.get(i));
  if (!*id) {
    JS_ReportErrorUTF8(cx, "TypeError: argument %u is not a Node", i + 1);
    return false;
  }
  return true;
}

// Throws `new DOMException(aMessage, aName)`, the prelude's class (ErrorResult::ThrowDOMException);
// false, for the native to return.
static bool ThrowNamedDOMException(JSContext* cx, const char* aName, const char* aMessage) {
  JS::RootedObject global(cx, JS::CurrentGlobalOrNull(cx));
  JS::RootedValue ctor(cx);
  if (!JS_GetProperty(cx, global, "DOMException", &ctor) || !ctor.isObject()) {
    JS_ReportErrorUTF8(cx, "%s: %s", aName, aMessage);
    return false;
  }
  JS::RootedValueArray<2> ctorArgs(cx);
  JS::RootedString message(cx, JS_NewStringCopyZ(cx, aMessage)), name(cx, JS_NewStringCopyZ(cx, aName));
  if (!message || !name) return false;
  ctorArgs[0].setString(message);
  ctorArgs[1].setString(name);
  JS::RootedObject exception(cx);
  if (!JS::Construct(cx, ctor, ctorArgs, &exception)) return false;
  JS::RootedValue v(cx, JS::ObjectValue(*exception));
  JS_SetPendingException(cx, v);
  return false;
}

static bool ThrowDOMException(JSContext* cx, int32_t code) {
  JS_ReportErrorUTF8(cx, "%s", code == 1 ? "HierarchyRequestError: the operation would yield an incorrect node tree"
                                         : "NotFoundError: the node is not a child of this node");
  return false;
}

// ---- custom elements: upgrades and reactions (dom/base/CustomElementRegistry.cpp) ----
//
// Translated from Gecko, with its names. The DOM side is the node's custom element state
// (mrt_dom_ce_state); everything else is here. See DESIGN "Custom elements, as built".

// Connected: the shadow-including root is the page's document (a shadow tree is connected when its
// host is).
static bool IsConnected(uint32_t aNode) {
  while (uint32_t parent = mrt_dom_link(tJs->host, aNode, LINK_SHADOW_INCLUDING_PARENT)) aNode = parent;
  return aNode == 1;
}

static std::string LocalName(uint32_t aElement) {
  const char* s;
  size_t len;
  mrt_dom_name(tJs->host, aElement, &s, &len);
  return std::string(s, len);
}

// The next node of a subtree in shadow-including tree order, or 0 past its end: a host's shadow
// root and its tree come right after the host, before the host's children (the DOM's
// "shadow-including descendant", which the custom element steps walk). Template contents are not in
// it.
static uint32_t NextInSubtree(uint32_t aNode, uint32_t aRoot) {
  void* h = tJs->host;
  if (uint32_t root = mrt_dom_link(h, aNode, LINK_SHADOW_ROOT)) return root;
  if (uint32_t first = mrt_dom_link(h, aNode, LINK_FIRST)) return first;
  uint32_t n = aNode;
  while (n && n != aRoot) {
    if (uint32_t next = mrt_dom_link(h, n, LINK_NEXT)) return next;
    uint32_t parent = mrt_dom_link(h, n, LINK_PARENT);
    if (!parent) {
      // the end of a shadow tree: on to its host's children
      uint32_t host = mrt_dom_link(h, n, LINK_HOST);
      if (!host) return 0;
      if (uint32_t first = mrt_dom_link(h, host, LINK_FIRST)) return first;
      parent = host;
    }
    n = parent;
  }
  return 0;
}

// The next node in tree order inside one tree (no shadow trees), or 0 past the subtree's end.
static uint32_t NextInTree(uint32_t aNode, uint32_t aRoot) {
  if (uint32_t first = mrt_dom_link(tJs->host, aNode, LINK_FIRST)) return first;
  for (uint32_t n = aNode; n && n != aRoot; n = mrt_dom_link(tJs->host, n, LINK_PARENT)) {
    if (uint32_t next = mrt_dom_link(tJs->host, n, LINK_NEXT)) return next;
  }
  return 0;
}

static CustomElementDefinition* GetCustomElementDefinition(uint32_t aElement) {
  auto it = tJs->ce.mElementDefinition.find(aElement);
  return it == tJs->ce.mElementDefinition.end() ? nullptr : it->second;
}

// CustomElementRegistry::LookupCustomElementDefinition by local name (HTML namespace, no `is`).
static CustomElementDefinition* LookupCustomElementDefinition(const std::string& aLocalName) {
  auto it = tJs->ce.mCustomDefinitions.find(aLocalName);
  return it == tJs->ce.mCustomDefinitions.end() ? nullptr : it->second;
}

// CustomElementRegistry::LookupCustomElementDefinition by constructor. A scan: a page defines a few
// hundred at most, and a moving GC would invalidate a map keyed by the object's address.
static CustomElementDefinition* LookupCustomElementDefinition(JSObject* aConstructor) {
  for (auto& def : tJs->ce.mDefinitions) {
    if (def->mConstructor == aConstructor) return def.get();
  }
  return nullptr;
}

// An exception from a reaction or a custom element constructor goes to the console and not to the
// caller, as Gecko reports it (AutoEntryScript::ReportException).
static void ReportCustomElementException(JSContext* cx, const char* aWhere) {
  JS::RootedValue exn(cx);
  if (!JS_GetPendingException(cx, &exn)) {
    JS_ClearPendingException(cx);
    return;
  }
  JS_ClearPendingException(cx);
  std::string line = std::string("uncaught exception in a custom element ") + aWhere + ": ", part;
  if (ArgUTF8(cx, exn, part)) line += part;
  JS::RootedValue stack(cx);
  if (exn.isObject()) {
    JS::RootedObject obj(cx, &exn.toObject());
    if (JS_GetProperty(cx, obj, "stack", &stack) && stack.isString() && ArgUTF8(cx, stack, part)) line += "\n" + part;
  }
  JS_ClearPendingException(cx);
  mrt_console(tJs->host, line.data(), line.size());
}

static void InvokeBackupQueue(JSContext* cx);

// BackupQueueMicroTask: the backup element queue is run as a microtask.
static bool BackupQueueMicroTask_Run(JSContext* cx, unsigned argc, JS::Value* vp) {
  JS::CallArgs args = JS::CallArgsFromVp(argc, vp);
  InvokeBackupQueue(cx);
  tJs->ce.mIsBackupQueueProcessing = false;
  args.rval().setUndefined();
  return true;
}

static void DispatchBackupQueueMicroTask(JSContext* cx) {
  tJs->ce.mIsBackupQueueProcessing = true;
  JS::RootedObject promise(cx, JS::CallOriginalPromiseResolve(cx, JS::UndefinedHandleValue));
  JSFunction* fun = JS_NewFunction(cx, BackupQueueMicroTask_Run, 1, 0, "BackupQueueMicroTask");
  JS::RootedObject run(cx, fun ? JS_GetFunctionObject(fun) : nullptr);
  if (!promise || !run || !JS::AddPromiseReactions(cx, promise, run, run)) {
    // no microtask (out of memory): run the queue now rather than never
    JS_ClearPendingException(cx);
    InvokeBackupQueue(cx);
    tJs->ce.mIsBackupQueueProcessing = false;
  }
}

static void CreateAndPushElementQueue() {
  CustomElements& ce = tJs->ce;
  ce.mReactionsStack.emplace_back();
  ce.mIsElementQueuePushedForCurrentRecursionDepth = true;
}

// CustomElementReactionsStack::Enqueue: the reaction to the element's queue, the element to the
// current element queue, or to the backup queue when no [CEReactions] method is running.
static void Enqueue(uint32_t aElement, CustomElementReaction aReaction) {
  CustomElements& ce = tJs->ce;
  if (ce.mRecursionDepth) {
    if (!ce.mIsElementQueuePushedForCurrentRecursionDepth) CreateAndPushElementQueue();
    ce.mReactionsStack.back().push_back(aElement);
    ce.mReactionQueue[aElement].push_back(std::move(aReaction));
    return;
  }
  ce.mBackupQueue.push_back(aElement);
  ce.mReactionQueue[aElement].push_back(std::move(aReaction));
  if (ce.mIsBackupQueueProcessing) return;
  DispatchBackupQueueMicroTask(tJs->cx);
}

static void EnqueueUpgradeReaction(uint32_t aElement, CustomElementDefinition* aDefinition) {
  Enqueue(aElement, CustomElementReaction{aDefinition, true, ElementCallbackType::eConnected, {}});
}

static JSObject* CallbackOf(CustomElementDefinition* aDefinition, ElementCallbackType aType) {
  switch (aType) {
    case ElementCallbackType::eConnected: return aDefinition->mConnectedCallback;
    case ElementCallbackType::eDisconnected: return aDefinition->mDisconnectedCallback;
    case ElementCallbackType::eAdopted: return aDefinition->mAdoptedCallback;
    case ElementCallbackType::eAttributeChanged: return aDefinition->mAttributeChangedCallback;
  }
  return nullptr;
}

// CustomElementRegistry::EnqueueLifecycleCallback. For attributeChangedCallback the caller checks
// the observed attributes.
static void EnqueueLifecycleCallback(ElementCallbackType aType, uint32_t aCustomElement, LifecycleCallbackArgs aArgs,
                                     CustomElementDefinition* aDefinition = nullptr) {
  CustomElementDefinition* definition = aDefinition;
  if (!definition) {
    definition = GetCustomElementDefinition(aCustomElement);
    if (!definition || definition->mLocalName != LocalName(aCustomElement)) return;
  }
  if (!CallbackOf(definition, aType)) return;
  Enqueue(aCustomElement, CustomElementReaction{definition, false, aType, std::move(aArgs)});
}

// nsContentUtils::TryToUpgradeElement: an undefined element that became connected. Gecko also
// registers it as a candidate for a later define; the mini runtime finds those by walking the
// document when the name is defined (UpgradeCandidates).
static void TryToUpgradeElement(uint32_t aElement) {
  if (CustomElementDefinition* definition = LookupCustomElementDefinition(LocalName(aElement))) {
    EnqueueUpgradeReaction(aElement, definition);
  }
}

// Element::BindToTree's custom element step for each element of a subtree that became connected,
// in tree order: connectedCallback for a custom element, an upgrade try for an undefined one.
static void EnqueueConnectedReactions(uint32_t aRoot) {
  if (tJs->ce.mDefinitions.empty()) return;  // no custom element can exist and none can upgrade
  for (uint32_t n = aRoot; n; n = NextInSubtree(n, aRoot)) {
    int32_t state = mrt_dom_ce_state(tJs->host, n);
    if (state == CE_CUSTOM) {
      EnqueueLifecycleCallback(ElementCallbackType::eConnected, n, {});
    } else if (state == CE_UNDEFINED) {
      TryToUpgradeElement(n);
    }
  }
}

// Element::UnbindFromTree's: disconnectedCallback for each custom element of a subtree that was
// connected, in tree order.
static void EnqueueDisconnectedReactions(uint32_t aRoot) {
  if (tJs->ce.mDefinitions.empty()) return;
  for (uint32_t n = aRoot; n; n = NextInSubtree(n, aRoot)) {
    if (mrt_dom_ce_state(tJs->host, n) == CE_CUSTOM) {
      EnqueueLifecycleCallback(ElementCallbackType::eDisconnected, n, {});
    }
  }
}

// Element::SetAttrAndNotify and UnsetAttr's step: attributeChangedCallback for an observed
// attribute of a custom element. Gecko enqueues it even when the value did not change.
static void EnqueueAttributeChanged(uint32_t aElement, const std::string& aName, std::optional<std::string> aOldValue,
                                    std::optional<std::string> aNewValue) {
  if (mrt_dom_ce_state(tJs->host, aElement) != CE_CUSTOM) return;
  CustomElementDefinition* definition = GetCustomElementDefinition(aElement);
  if (!definition || !definition->IsInObservedAttributeList(aName)) return;
  EnqueueLifecycleCallback(ElementCallbackType::eAttributeChanged, aElement,
                           LifecycleCallbackArgs{aName, std::move(aOldValue), std::move(aNewValue)}, definition);
}

// Elements made by fragment parsing or cloning whose name is defined get an upgrade reaction, as
// Gecko's non-synchronous element creation gives them (NewXULOrHTMLElement step 5.2).
static void EnqueueUpgradesForNewSubtree(uint32_t aRoot) {
  if (tJs->ce.mDefinitions.empty()) return;
  for (uint32_t n = aRoot; n; n = NextInSubtree(n, aRoot)) {
    if (mrt_dom_ce_state(tJs->host, n) != CE_UNDEFINED) continue;
    if (CustomElementDefinition* definition = LookupCustomElementDefinition(LocalName(n))) {
      EnqueueUpgradeReaction(n, definition);
    }
  }
}

// The element's lifecycle callback, called with the element as `this` (CustomElementCallback::Call).
static bool CallCustomElementCallback(JSContext* cx, uint32_t aElement, const CustomElementReaction& aReaction) {
  JS::RootedObject func(cx, CallbackOf(aReaction.mDefinition, aReaction.mType));
  if (!func) return true;
  JS::RootedValue thisv(cx), rval(cx);
  if (!Wrap(cx, aElement, &thisv)) return false;
  JS::RootedValueArray<4> argv(cx);
  size_t argc = 0;
  if (aReaction.mType == ElementCallbackType::eAttributeChanged) {
    const LifecycleCallbackArgs& a = aReaction.mArgs;
    if (!NewString(cx, a.mName.data(), a.mName.size(), argv[0])) return false;
    if (a.mOldValue) {
      if (!NewString(cx, a.mOldValue->data(), a.mOldValue->size(), argv[1])) return false;
    } else {
      argv[1].setNull();
    }
    if (a.mNewValue) {
      if (!NewString(cx, a.mNewValue->data(), a.mNewValue->size(), argv[2])) return false;
    } else {
      argv[2].setNull();
    }
    argv[3].setNull();  // the namespace
    argc = 4;
  }
  JS::RootedValue fval(cx, JS::ObjectValue(*func));
  return JS::Call(cx, thisv, fval, JS::HandleValueArray::subarray(argv, 0, argc), &rval);
}

// DoUpgrade (CustomElementRegistry.cpp): run the constructor on an element being upgraded.
static bool DoUpgrade(JSContext* cx, uint32_t aElement, CustomElementDefinition* aDefinition) {
  // 9.1 (disable shadow with a shadow root already attached) needs shadow roots: not yet.
  mrt_dom_set_ce_state(tJs->host, aElement, CE_PRECUSTOMIZED);
  JS::RootedValue constructor(cx, JS::ObjectValue(*aDefinition->mConstructor));
  JS::RootedObject constructResult(cx);
  if (!JS::Construct(cx, constructor, JS::HandleValueArray::empty(), &constructResult)) return false;
  JS::RootedValue result(cx, JS::ObjectValue(*constructResult));
  if (NodeOf(result) != aElement) {
    JS_ReportErrorUTF8(cx, "TypeError: Custom element constructor returned a wrong element");
    return false;
  }
  return true;
}

// CustomElementRegistry::Upgrade (https://html.spec.whatwg.org/#concept-upgrade-an-element).
static bool Upgrade(JSContext* cx, uint32_t aElement, CustomElementDefinition* aDefinition) {
  CustomElements& ce = tJs->ce;
  // 1. Only an undefined element.
  if (mrt_dom_ce_state(tJs->host, aElement) != CE_UNDEFINED) return true;
  // 2-3. The definition, and "failed" until the constructor succeeds.
  ce.mElementDefinition[aElement] = aDefinition;
  mrt_dom_set_ce_state(tJs->host, aElement, CE_FAILED);
  // 4. attributeChangedCallback for each observed attribute it already has, in order.
  if (!aDefinition->mObservedAttributes.empty()) {
    const char* s;
    size_t len;
    mrt_dom_attr_names(tJs->host, aElement, &s, &len);
    std::string names(s, len);
    size_t start = 0;
    while (start < names.size()) {
      size_t end = names.find('\n', start);
      if (end == std::string::npos) end = names.size();
      std::string name = names.substr(start, end - start);
      start = end + 1;
      if (name.empty() || !aDefinition->IsInObservedAttributeList(name)) continue;
      const char* v;
      size_t vlen;
      if (!mrt_dom_get_attr(tJs->host, aElement, name.data(), name.size(), &v, &vlen)) continue;
      EnqueueLifecycleCallback(ElementCallbackType::eAttributeChanged, aElement,
                               LifecycleCallbackArgs{name, std::nullopt, std::string(v, vlen)}, aDefinition);
    }
  }
  // 5. connectedCallback if it is connected.
  if (IsConnected(aElement)) EnqueueLifecycleCallback(ElementCallbackType::eConnected, aElement, {}, aDefinition);
  // 6-9. On the construction stack while its constructor runs (AutoConstructionStackEntry).
  aDefinition->mConstructionStack.push_back(aElement);
  bool ok = DoUpgrade(cx, aElement, aDefinition);
  aDefinition->mConstructionStack.pop_back();
  if (!ok) {
    // failed, without its definition, its reaction queue emptied (Gecko's addition to the spec)
    mrt_dom_set_ce_state(tJs->host, aElement, CE_FAILED);
    ce.mElementDefinition.erase(aElement);
    ce.mReactionQueue.erase(aElement);
    return false;
  }
  // 11. Custom (and :defined).
  mrt_dom_set_ce_state(tJs->host, aElement, CE_CUSTOM);
  return true;
}

// CustomElementReactionsStack::InvokeReactions. `aQueue` is fetched again after every reaction,
// because reactions can push queues of their own (and so move the stack's storage).
template <typename QueueOf>
static void InvokeReactions(JSContext* cx, QueueOf aQueue) {
  CustomElements& ce = tJs->ce;
  for (size_t i = 0; i < aQueue().size(); ++i) {
    uint32_t element = aQueue()[i];
    for (size_t j = 0;; ++j) {
      auto it = ce.mReactionQueue.find(element);
      if (it == ce.mReactionQueue.end() || j >= it->second.size()) break;
      // taken out of the queue first: a reaction can run this function again for the element
      CustomElementReaction reaction = std::move(it->second[j]);
      it->second[j].mDefinition = nullptr;
      if (!reaction.mDefinition) continue;
      bool ok = reaction.mIsUpgrade ? Upgrade(cx, element, reaction.mDefinition)
                                    : CallCustomElementCallback(cx, element, reaction);
      if (!ok) ReportCustomElementException(cx, reaction.mIsUpgrade ? "upgrade" : "callback");
    }
    ce.mReactionQueue.erase(element);
  }
  aQueue().clear();
}

static void InvokeBackupQueue(JSContext* cx) {
  if (!tJs->ce.mBackupQueue.empty()) {
    InvokeReactions(cx, [] () -> std::vector<uint32_t>& { return tJs->ce.mBackupQueue; });
  }
}

static void PopAndInvokeElementQueue(JSContext* cx) {
  CustomElements& ce = tJs->ce;
  size_t lastIndex = ce.mReactionsStack.size() - 1;
  if (!ce.mReactionsStack[lastIndex].empty()) {
    InvokeReactions(cx, [lastIndex] () -> std::vector<uint32_t>& { return tJs->ce.mReactionsStack[lastIndex]; });
  }
  ce.mReactionsStack.pop_back();
  ce.mIsElementQueuePushedForCurrentRecursionDepth = false;
}

static bool DoCustomElementCreate(JSContext* cx, CustomElementDefinition* aDefinition, JS::MutableHandleValue rval);

// [CEReactions] (CustomElementReactionsStack::EnterCEReactions and LeaveCEReactions): a native
// that can enqueue reactions runs them before it returns. Inactive inside a bridge.js method made
// of several changes (__mrtCEReactions), whose own scope runs them once at its end.
class AutoCEReaction {
 public:
  explicit AutoCEReaction(JSContext* cx) : mCx(cx), mActive(!tJs->ce.mInCompoundMethod) {
    if (!mActive) return;
    CustomElements& ce = tJs->ce;
    mWasElementQueuePushed = ce.mIsElementQueuePushedForCurrentRecursionDepth;
    ce.mRecursionDepth++;
    ce.mIsElementQueuePushedForCurrentRecursionDepth = false;
  }
  ~AutoCEReaction() {
    if (!mActive) return;
    CustomElements& ce = tJs->ce;
    if (ce.mIsElementQueuePushedForCurrentRecursionDepth) {
      JS::AutoSaveExceptionState saved(mCx);  // the native's own exception survives the reactions
      PopAndInvokeElementQueue(mCx);
    }
    ce.mRecursionDepth--;
    ce.mIsElementQueuePushedForCurrentRecursionDepth = mWasElementQueuePushed;
  }

 private:
  JSContext* mCx;
  bool mActive;
  bool mWasElementQueuePushed = false;
};

// ---- Node ----

// MRT_LOOP_TRACE: the time of each native the page calls, the outermost only (a native that calls a
// script that calls another counts both in the first), for realsite's long-task lines
// (mrt_js_take_native_times, task 9caa0). Off, a native pays one load and a branch.
struct NativeStat {
  const char* mName;
  uint64_t mCalls = 0;
  uint64_t mNs = 0;
  NativeStat* mNext = nullptr;
};
static NativeStat* sNativeStats = nullptr;
static bool sNativeTiming = getenv("MRT_LOOP_TRACE") != nullptr;
// (the page's thread only, the one that takes the times: workers run the same natives on threads of
// their own)
static pthread_t sNativeThread;
static bool sNativeThreadKnown = false;
static thread_local int sNativeDepth = 0;
struct NativeStatList {
  explicit NativeStatList(NativeStat* aStat) {
    aStat->mNext = sNativeStats;
    sNativeStats = aStat;
  }
};
static uint64_t NativeNowNs() {
  timespec ts;
  clock_gettime(CLOCK_MONOTONIC, &ts);
  return uint64_t(ts.tv_sec) * 1000000000 + ts.tv_nsec;
}
struct NativeTimer {
  NativeStat* mStat;
  uint64_t mStart = 0;
  bool mOutermost = false;
  explicit NativeTimer(NativeStat* aStat) : mStat(aStat) {
    if (sNativeTiming && sNativeDepth++ == 0 && sNativeThreadKnown && pthread_equal(pthread_self(), sNativeThread)) {
      mOutermost = true;
      mStart = NativeNowNs();
    }
  }
  ~NativeTimer() {
    if (!sNativeTiming) return;
    sNativeDepth--;
    if (mOutermost) {
      mStat->mCalls++;
      mStat->mNs += NativeNowNs() - mStart;
    }
  }
};
#define NATIVE_TIMED(name)                                 \
  static NativeStat name##_stat{#name};                    \
  static NativeStatList name##_listed(&name##_stat);       \
  NativeTimer name##_timer(&name##_stat)
#define NATIVE(name)                                               \
  static bool name##_body(JSContext* cx, unsigned argc, JS::Value* vp); \
  static bool name(JSContext* cx, unsigned argc, JS::Value* vp) {  \
    NATIVE_TIMED(name);                                            \
    return name##_body(cx, argc, vp);                              \
  }                                                                \
  static bool name##_body(JSContext* cx, unsigned argc, JS::Value* vp)

// The natives timed since the last call, the costliest first, as "name calls ms" lines (at most `max`),
// with "natives calls ms" for all of them first; the counts start again. Empty when not timing.
extern "C" size_t mrt_js_take_native_times(char* aOut, size_t aLen, size_t aMax) {
  if (!sNativeThreadKnown) {
    sNativeThread = pthread_self();
    sNativeThreadKnown = true;
  }
  std::vector<NativeStat*> stats;
  uint64_t calls = 0, ns = 0;
  for (NativeStat* s = sNativeStats; s; s = s->mNext) {
    if (!s->mCalls) continue;
    stats.push_back(s);
    calls += s->mCalls;
    ns += s->mNs;
  }
  std::sort(stats.begin(), stats.end(), [](NativeStat* a, NativeStat* b) { return a->mNs > b->mNs; });
  std::string out;
  char line[160];
  snprintf(line, sizeof line, "natives %llu %.1f", (unsigned long long)calls, ns / 1e6);
  out += line;
  for (size_t i = 0; i < stats.size() && i < aMax; i++) {
    snprintf(line, sizeof line, "\n%s %llu %.1f", stats[i]->mName, (unsigned long long)stats[i]->mCalls, stats[i]->mNs / 1e6);
    out += line;
  }
  for (NativeStat* s : stats) s->mCalls = s->mNs = 0;
  size_t n = std::min(out.size(), aLen);
  memcpy(aOut, out.data(), n);
  return n;
}
#define THIS_NODE                              \
  JS::CallArgs args = JS::CallArgsFromVp(argc, vp); \
  uint32_t self;                               \
  if (!ThisNode(cx, args, &self)) return false

static bool Link(JSContext* cx, unsigned argc, JS::Value* vp, int which, bool elements_only) {
  THIS_NODE;
  uint32_t id = mrt_dom_link(tJs->host, self, which);
  if (elements_only) {
    // nextElementSibling and friends: skip to the next element in the same direction
    int step = which == 1 ? 3 : which == 2 ? 4 : which;
    while (id && mrt_dom_kind(tJs->host, id) != KIND_ELEMENT) id = mrt_dom_link(tJs->host, id, step);
  }
  return Wrap(cx, id, args.rval());
}

NATIVE(Node_parentNode) { return Link(cx, argc, vp, 0, false); }
NATIVE(Node_firstChild) { return Link(cx, argc, vp, 1, false); }
NATIVE(Node_lastChild) { return Link(cx, argc, vp, 2, false); }
NATIVE(Node_nextSibling) { return Link(cx, argc, vp, 3, false); }
NATIVE(Node_previousSibling) { return Link(cx, argc, vp, 4, false); }
NATIVE(Node_parentElement) {
  THIS_NODE;
  uint32_t p = mrt_dom_link(tJs->host, self, 0);
  return Wrap(cx, p && mrt_dom_kind(tJs->host, p) == KIND_ELEMENT ? p : 0, args.rval());
}
NATIVE(Element_firstElementChild) { return Link(cx, argc, vp, 1, true); }
NATIVE(Element_lastElementChild) { return Link(cx, argc, vp, 2, true); }
NATIVE(Element_nextElementSibling) { return Link(cx, argc, vp, 3, true); }
NATIVE(Element_previousElementSibling) { return Link(cx, argc, vp, 4, true); }

NATIVE(Node_nodeType) {
  THIS_NODE;
  static const int kTypes[] = {9, 1, 3, 8, 10, 11, 11};
  args.rval().setInt32(kTypes[mrt_dom_kind(tJs->host, self)]);
  return true;
}

// nodeName and tagName: the tag uppercased for HTML elements (Element::NodeName).
static bool NodeName(JSContext* cx, JS::CallArgs& args, uint32_t self) {
  switch (mrt_dom_kind(tJs->host, self)) {
    case KIND_DOCUMENT: return NewString(cx, "#document", 9, args.rval());
    case KIND_TEXT: return NewString(cx, "#text", 5, args.rval());
    case KIND_COMMENT: return NewString(cx, "#comment", 8, args.rval());
    case KIND_FRAGMENT:
    case KIND_SHADOW_ROOT: return NewString(cx, "#document-fragment", 18, args.rval());
    default: break;
  }
  const char* s;
  size_t len;
  int32_t ns = mrt_dom_name(tJs->host, self, &s, &len);
  std::string name(s, len);
  if (ns == NS_HTML) {
    for (char& c : name) {
      if (c >= 'a' && c <= 'z') c -= 32;
    }
  }
  return NewString(cx, name.data(), name.size(), args.rval());
}
NATIVE(Node_nodeName) {
  THIS_NODE;
  return NodeName(cx, args, self);
}

NATIVE(Node_textContent) {
  THIS_NODE;
  int kind = mrt_dom_kind(tJs->host, self);
  if (kind == KIND_DOCUMENT || kind == KIND_DOCTYPE) {
    args.rval().setNull();
    return true;
  }
  const char* s;
  size_t len;
  mrt_dom_text_content(tJs->host, self, &s, &len);
  return NewString(cx, s, len, args.rval());
}
static void SetTextContent(JSContext* cx, uint32_t aNode, const std::string& aValue);
NATIVE(Node_set_textContent) {
  THIS_NODE;
  AutoCEReaction ceReaction(cx);
  std::string text;
  if (!args.get(0).isNull() && !ArgUTF8(cx, args.get(0), text)) return false;
  SetTextContent(cx, self, text);
  args.rval().setUndefined();
  return true;
}

// childNodes and children: a new array each time (Firefox returns a live NodeList).
static bool ChildArray(JSContext* cx, unsigned argc, JS::Value* vp, bool elements_only) {
  THIS_NODE;
  JS::RootedObject arr(cx, JS::NewArrayObject(cx, 0));
  if (!arr) return false;
  JS::RootedValue v(cx);
  uint32_t i = 0;
  for (uint32_t c = mrt_dom_link(tJs->host, self, 1); c; c = mrt_dom_link(tJs->host, c, 3)) {
    if (elements_only && mrt_dom_kind(tJs->host, c) != KIND_ELEMENT) continue;
    if (!Wrap(cx, c, &v) || !JS_SetElement(cx, arr, i++, v)) return false;
  }
  args.rval().setObject(*arr);
  return true;
}
NATIVE(Node_childNodes) { return ChildArray(cx, argc, vp, false); }
NATIVE(Element_children) { return ChildArray(cx, argc, vp, true); }
NATIVE(Element_childElementCount) {
  THIS_NODE;
  int32_t n = 0;
  for (uint32_t c = mrt_dom_link(tJs->host, self, 1); c; c = mrt_dom_link(tJs->host, c, 3)) {
    n += mrt_dom_kind(tJs->host, c) == KIND_ELEMENT;
  }
  args.rval().setInt32(n);
  return true;
}

// The node's document (nsINode::OwnerDoc: the document that made it or that it was adopted into);
// a document's own is null.
NATIVE(Node_ownerDocument) {
  THIS_NODE;
  if (mrt_dom_kind(tJs->host, self) == KIND_DOCUMENT) return Wrap(cx, 0, args.rval());
  return Wrap(cx, mrt_dom_node_document(tJs->host, self), args.rval());
}
// isConnected: the node's shadow-including root is a document, any document (nsINode::IsInComposedDoc).
// IsConnected above is the page's document only: a document made by createHTMLDocument or DOMParser
// has no browsing context, so no custom element registry and no reactions, as in Gecko.
NATIVE(Node_isConnected) {
  THIS_NODE;
  uint32_t root = self;
  while (uint32_t parent = mrt_dom_link(tJs->host, root, LINK_SHADOW_INCLUDING_PARENT)) root = parent;
  args.rval().setBoolean(mrt_dom_kind(tJs->host, root) == KIND_DOCUMENT);
  return true;
}
// getRootNode(options): the root of the node's tree; with `composed`, out of every shadow tree
// through its host (nsINode::GetRootNode).
NATIVE(Node_getRootNode) {
  THIS_NODE;
  bool composed = false;
  if (args.get(0).isObject()) {
    JS::RootedObject opts(cx, &args[0].toObject());
    JS::RootedValue v(cx);
    if (!JS_GetProperty(cx, opts, "composed", &v)) return false;
    composed = JS::ToBoolean(v);
  }
  uint32_t root = self;
  for (;;) {
    while (uint32_t parent = mrt_dom_link(tJs->host, root, LINK_PARENT)) root = parent;
    uint32_t host = composed ? mrt_dom_link(tJs->host, root, LINK_HOST) : 0;
    if (!host) break;
    root = host;
  }
  return Wrap(cx, root, args.rval());
}

// ---- shadow DOM: slot changes (dom/base/ShadowRoot.cpp, dom/html/HTMLSlotElement.cpp) ----
//
// Gecko keeps each slot's assigned nodes up to date on every change and signals a slot whose list
// changed (HTMLSlotElement::EnqueueSlotChangeEvent): a `slotchange` event at it in the mutation
// observer microtask. The mini runtime computes assignment when asked (mrt_dom_link
// LINK_ASSIGNED_SLOT), so it finds the changed slots by comparing the slots of the shadow trees a
// change can touch before and after it: the host's tree when a host's child or its `slot` attribute
// changes, and the containing tree when a slot is inserted, removed or renamed.

using SlotAssignments = std::vector<std::pair<uint32_t, std::vector<uint32_t>>>;

static bool IsSlot(uint32_t aNode) {
  if (mrt_dom_kind(tJs->host, aNode) != KIND_ELEMENT) return false;
  const char* s;
  size_t len;
  return mrt_dom_name(tJs->host, aNode, &s, &len) == NS_HTML && len == 4 && !memcmp(s, "slot", 4);
}

// The shadow root at the top of the node's tree, or 0.
static uint32_t ContainingShadow(uint32_t aNode) {
  while (uint32_t parent = mrt_dom_link(tJs->host, aNode, LINK_PARENT)) aNode = parent;
  return mrt_dom_kind(tJs->host, aNode) == KIND_SHADOW_ROOT ? aNode : 0;
}

static bool SubtreeHasSlot(uint32_t aRoot) {
  for (uint32_t n = aRoot; n; n = NextInTree(n, aRoot)) {
    if (IsSlot(n)) return true;
  }
  return false;
}

// Each slot of the shadow tree with its assigned nodes.
static SlotAssignments SnapshotSlots(uint32_t aShadowRoot) {
  SlotAssignments out;
  int32_t n = mrt_dom_slots(tJs->host, aShadowRoot);
  for (int32_t i = 0; i < n; i++) out.push_back({mrt_dom_result(tJs->host, i), {}});
  for (auto& [slot, nodes] : out) {
    int32_t m = mrt_dom_assigned_nodes(tJs->host, slot);
    for (int32_t i = 0; i < m; i++) nodes.push_back(mrt_dom_result(tJs->host, i));
  }
  return out;
}

static void SignalSlotChange(JSContext* cx, uint32_t aSlot);

// The shadow trees a change may reassign, with their slots before it; Finish signals the slots
// whose assigned nodes differ after it.
struct SlotChangeWatch {
  std::vector<std::pair<uint32_t, SlotAssignments>> mTrees;
  void Watch(uint32_t aShadowRoot) {
    if (!aShadowRoot) return;
    for (auto& t : mTrees) {
      if (t.first == aShadowRoot) return;
    }
    mTrees.push_back({aShadowRoot, SnapshotSlots(aShadowRoot)});
  }
  // A change of `aParent`'s children that moves `aSubtree` (inserted or removed).
  void WatchChildren(uint32_t aParent, uint32_t aSubtree) {
    Watch(mrt_dom_link(tJs->host, aParent, LINK_SHADOW_ROOT));
    if (uint32_t shadow = ContainingShadow(aParent); shadow && SubtreeHasSlot(aSubtree)) Watch(shadow);
  }
  void Finish(JSContext* cx) {
    for (auto& [root, before] : mTrees) {
      SlotAssignments after = SnapshotSlots(root);
      for (auto& [slot, nodes] : after) {
        auto it = std::find_if(before.begin(), before.end(), [&](auto& b) { return b.first == slot; });
        if (it == before.end() ? !nodes.empty() : it->second != nodes) SignalSlotChange(cx, slot);
      }
      for (auto& [slot, nodes] : before) {
        bool gone = std::none_of(after.begin(), after.end(), [&](auto& a) { return a.first == slot; });
        if (gone && !nodes.empty()) SignalSlotChange(cx, slot);
      }
    }
  }
};

// An exception thrown by a callback the runtime calls (Gecko's CallbackObject reports it).
static void ReportCallbackException(JSContext* cx, const char* aWhat) {
  JS::RootedValue exn(cx);
  if (!JS_GetPendingException(cx, &exn)) {
    JS_ClearPendingException(cx);
    return;
  }
  JS_ClearPendingException(cx);
  std::string line = std::string("uncaught exception in a ") + aWhat + ": ", part;
  if (ArgUTF8(cx, exn, part)) line += part;
  JS::RootedValue stack(cx);
  if (exn.isObject()) {
    JS::RootedObject obj(cx, &exn.toObject());
    if (JS_GetProperty(cx, obj, "stack", &stack) && stack.isString() && ArgUTF8(cx, stack, part)) line += "\n" + part;
  }
  JS_ClearPendingException(cx);
  mrt_console(tJs->host, line.data(), line.size());
}

// ---- mutation observers (dom/base/nsDOMMutationObserver.cpp, MutationObservers.cpp) ----
//
// Translated from Gecko with its names (DESIGN "MutationObserver, as built"). The notifications
// are made from the DOM funnels below (Insert, removeChild, SetAttr, removeAttribute, textContent,
// adoptNode and the batches bridge.js opens for innerHTML and replaceChildren), where Gecko's
// MutationObservers::Notify* calls are made from nsINode and Element.

static uint32_t ParentNode(uint32_t aNode) { return mrt_dom_link(tJs->host, aNode, LINK_PARENT); }
static uint32_t PreviousSibling(uint32_t aNode) { return mrt_dom_link(tJs->host, aNode, LINK_PREV); }
static uint32_t NextSibling(uint32_t aNode) { return mrt_dom_link(tJs->host, aNode, LINK_NEXT); }
static uint32_t FirstChild(uint32_t aNode) { return mrt_dom_link(tJs->host, aNode, LINK_FIRST); }
static uint32_t LastChild(uint32_t aNode) { return mrt_dom_link(tJs->host, aNode, LINK_LAST); }

// nsINode::SubtreeRoot: the document, the shadow root, or the root of a detached tree.
static uint32_t SubtreeRoot(uint32_t aNode) {
  while (uint32_t parent = ParentNode(aNode)) aNode = parent;
  return aNode;
}

// nsINode::AddMutationObserver, RemoveMutationObserver and SetMayHaveDOMMutationObserver
static void AddMutationObserver(nsMutationReceiver* r) {
  tJs->mo.mNodeObservers[r->mRegisterTarget].push_back(r);
  tJs->mo.mMayHaveDOMMutationObservers = true;
}
static void RemoveMutationObserver(nsMutationReceiver* r) {
  auto it = tJs->mo.mNodeObservers.find(r->mRegisterTarget);
  if (it == tJs->mo.mNodeObservers.end()) return;
  auto& list = it->second;
  list.erase(std::remove(list.begin(), list.end(), r), list.end());
  if (list.empty()) tJs->mo.mNodeObservers.erase(it);
}

// nsMutationReceiver::RemoveClones and DisconnectTransientReceiver
static void RemoveClones(nsMutationReceiver* r) {
  for (nsMutationReceiver* clone : r->mTransientReceivers) {
    if (clone->mRegisterTarget) {
      RemoveMutationObserver(clone);
      clone->mRegisterTarget = 0;
    }
    clone->mParent = nullptr;
  }
  r->mTransientReceivers.clear();
}

// nsMutationReceiver::Disconnect (the receiver is freed by its owner)
static void DisconnectReceiver(nsMutationReceiver* r) {
  if (r->mRegisterTarget) {
    RemoveMutationObserver(r);
    r->mRegisterTarget = 0;
  }
  r->mParent = nullptr;
  r->mTarget = 0;
  r->mObserver = nullptr;
  RemoveClones(r);
}

// nsDOMMutationObserver::GetReceiverFor (no animation receivers)
static nsMutationReceiver* GetReceiverFor(nsDOMMutationObserver* o, uint32_t aNode, bool aMayCreate) {
  for (auto& r : o->mReceivers) {
    if (r->Target() == aNode) return r.get();
  }
  if (!aMayCreate) return nullptr;
  auto r = std::make_unique<nsMutationReceiver>();
  r->mObserver = o;
  r->mTarget = aNode;
  r->mRegisterTarget = aNode;
  AddMutationObserver(r.get());
  o->mReceivers.push_back(std::move(r));
  return o->mReceivers.back().get();
}

// nsMutationReceiver::Create(aRegisterTarget, aParent): a transient receiver, kept by the
// observer's mTransientReceivers under its register target
static nsMutationReceiver* CreateTransientReceiver(nsDOMMutationObserver* o, uint32_t aRegisterTarget,
                                                   nsMutationReceiver* aParent) {
  auto r = std::make_unique<nsMutationReceiver>();
  r->mParent = aParent;
  r->mRegisterTarget = aRegisterTarget;
  aParent->mTransientReceivers.push_back(r.get());
  AddMutationObserver(r.get());
  for (auto& [node, list] : o->mTransientReceivers) {
    if (node == aRegisterTarget) {
      list.push_back(std::move(r));
      return list.back().get();
    }
  }
  o->mTransientReceivers.emplace_back(aRegisterTarget, std::vector<std::unique_ptr<nsMutationReceiver>>());
  o->mTransientReceivers.back().second.push_back(std::move(r));
  return o->mTransientReceivers.back().second.back().get();
}

static std::vector<std::unique_ptr<nsMutationReceiver>>* TransientReceiversOf(nsDOMMutationObserver* o, uint32_t aNode) {
  for (auto& [node, list] : o->mTransientReceivers) {
    if (node == aNode) return &list;
  }
  return nullptr;
}

// nsDOMMutationObserver::GetAllSubtreeObserversFor
static void GetAllSubtreeObserversFor(nsDOMMutationObserver* o, uint32_t aNode, std::vector<nsMutationReceiver*>& aReceivers) {
  auto contains = [&](nsMutationReceiver* r) { return std::find(aReceivers.begin(), aReceivers.end(), r) != aReceivers.end(); };
  for (uint32_t n = aNode; n; n = ParentNode(n)) {
    if (!tJs->mo.mNodeObservers.count(n)) continue;
    nsMutationReceiver* r = GetReceiverFor(o, n, false);
    if (r && r->Subtree() && !contains(r)) {
      aReceivers.push_back(r);
      if (o->mReceivers.size() == aReceivers.size()) return;
    }
    if (auto* transient = TransientReceiversOf(o, n)) {
      for (auto& t : *transient) {
        nsMutationReceiver* parent = t->GetParent();
        if (t->Subtree() && parent && !contains(parent)) aReceivers.push_back(parent);
      }
      if (o->mReceivers.size() == aReceivers.size()) return;
    }
  }
}

static bool MutationObserverMicroTask_Run(JSContext* cx, unsigned argc, JS::Value* vp);

// A microtask, queued as a promise reaction (the job queue runs it after the current script, in
// order with the page's own promise jobs). nsDOMMutationObserver::QueueMutationObserverMicroTask
static void QueueMutationObserverMicroTask(JSContext* cx) {
  JS::RootedObject promise(cx, JS::CallOriginalPromiseResolve(cx, JS::UndefinedHandleValue));
  JSFunction* fun = JS_NewFunction(cx, MutationObserverMicroTask_Run, 0, 0, "MutationObserverMicroTask");
  JS::RootedObject run(cx, fun ? JS_GetFunctionObject(fun) : nullptr);
  if (!promise || !run || !JS::AddPromiseReactions(cx, promise, run, run)) JS_ClearPendingException(cx);
}

// nsDOMMutationObserver::RescheduleForRun: the scheduled list is ordered by observer creation
static void RescheduleForRun(nsDOMMutationObserver* o) {
  auto& mo = tJs->mo;
  if (!mo.sScheduledMutationObservers) {
    QueueMutationObserverMicroTask(tJs->cx);
    mo.sScheduledMutationObservers.emplace();
  }
  auto& list = *mo.sScheduledMutationObservers;
  for (size_t i = 0; i < list.size(); i++) {
    if (list[i]->mId > o->mId) {
      list.insert(list.begin() + i, o);
      return;
    }
  }
  list.push_back(o);
}

// nsDOMMutationObserver::AddCurrentlyHandlingObserver
static void AddCurrentlyHandlingObserver(nsDOMMutationObserver* o, uint32_t aMutationLevel) {
  if (aMutationLevel > 1) AddCurrentlyHandlingObserver(o, aMutationLevel - 1);
  auto& handling = tJs->mo.sCurrentlyHandlingObservers;
  while (handling.size() < aMutationLevel) handling.emplace_back();
  auto& level = handling[aMutationLevel - 1];
  if (std::find(level.begin(), level.end(), o) == level.end()) level.push_back(o);
}

// nsDOMMutationObserver::ScheduleForRun
static void ScheduleForRun(nsDOMMutationObserver* o) {
  AddCurrentlyHandlingObserver(o, tJs->mo.sMutationLevel);
  if (o->mWaitingForRun) return;
  o->mWaitingForRun = true;
  RescheduleForRun(o);
}

// nsDOMMutationObserver::CurrentRecord: the record of the current mutation for this observer, made
// (and appended to the pending ones) by the first receiver that wants it
static nsDOMMutationRecord* CurrentRecord(nsDOMMutationObserver* o, const char* aType) {
  uint32_t level = tJs->mo.sMutationLevel;
  while (o->mCurrentMutations.size() < level) o->mCurrentMutations.push_back(nullptr);
  uint32_t last = level - 1;
  if (!o->mCurrentMutations[last]) {
    auto r = std::make_shared<nsDOMMutationRecord>();
    r->mType = aType;
    o->mCurrentMutations[last] = r;
    o->mPendingMutations.push_back(r);
    ScheduleForRun(o);
  }
  return o->mCurrentMutations[last].get();
}

// nsDOMMutationObserver::EnterMutationHandling and LeaveMutationHandling
static void EnterMutationHandling() { ++tJs->mo.sMutationLevel; }
static void LeaveMutationHandling() {
  auto& mo = tJs->mo;
  if (mo.sCurrentlyHandlingObservers.size() == mo.sMutationLevel && mo.sMutationLevel > 0) {
    std::vector<nsDOMMutationObserver*> obs = std::move(mo.sCurrentlyHandlingObservers.back());
    mo.sCurrentlyHandlingObservers.pop_back();
    for (nsDOMMutationObserver* o : obs) {
      if (o->mCurrentMutations.size() == mo.sMutationLevel) o->mCurrentMutations.pop_back();
    }
  }
  --mo.sMutationLevel;
}

// nsAutoMutationBatch: the changes made while it is open become one childList record per observer
// (removed nodes, then the nodes found between the previous and next siblings at the end).
class nsAutoMutationBatch {
 public:
  nsAutoMutationBatch() = default;
  nsAutoMutationBatch(uint32_t aTarget, bool aFromFirstToLast, bool aAllowNestedBatches) {
    Init(aTarget, aFromFirstToLast, aAllowNestedBatches);
  }
  ~nsAutoMutationBatch() { NodesAdded(); }

  void Init(uint32_t aTarget, bool aFromFirstToLast, bool aAllowNestedBatches) {
    auto& mo = tJs->mo;
    if (aTarget && mo.mMayHaveDOMMutationObservers) {
      if (mo.sCurrentBatch && !mo.sCurrentBatch->mAllowNestedBatches) return;
      mBatchTarget = aTarget;
      mFromFirstToLast = aFromFirstToLast;
      mAllowNestedBatches = aAllowNestedBatches;
      mPreviousBatch = mo.sCurrentBatch;
      mo.sCurrentBatch = this;
      EnterMutationHandling();
    }
  }
  void RemovalDone() { mRemovalDone = true; }
  static bool IsRemovalDone() { return tJs->mo.sCurrentBatch->mRemovalDone; }
  void SetPrevSibling(uint32_t aNode) { mPrevSibling = aNode; }
  void SetNextSibling(uint32_t aNode) { mNextSibling = aNode; }
  static bool IsBatching() { return tJs->mo.sCurrentBatch != nullptr; }
  static nsAutoMutationBatch* GetCurrentBatch() { return tJs->mo.sCurrentBatch; }
  static uint32_t GetBatchTarget() { return tJs->mo.sCurrentBatch->mBatchTarget; }

  static void UpdateObserver(nsDOMMutationObserver* aObserver, bool aWantsChildList) {
    auto& observers = tJs->mo.sCurrentBatch->mObservers;
    for (auto& bo : observers) {
      if (bo.first == aObserver) {
        if (aWantsChildList) bo.second = aWantsChildList;
        return;
      }
    }
    observers.push_back({aObserver, aWantsChildList});
  }

  static void NodeRemoved(uint32_t aChild) {
    nsAutoMutationBatch* b = tJs->mo.sCurrentBatch;
    if (b && !b->mRemovalDone) {
      if (b->mRemovedNodes.empty() || b->mRemovedNodes.back() != aChild) b->mRemovedNodes.push_back(aChild);
    }
  }

  void NodesAdded() {
    if (tJs->mo.sCurrentBatch != this) return;
    for (uint32_t c = mPrevSibling ? NextSibling(mPrevSibling) : FirstChild(mBatchTarget); c && c != mNextSibling;
         c = NextSibling(c)) {
      mAddedNodes.push_back(c);
    }
    Done();
  }

  void Done();

 private:
  nsAutoMutationBatch* mPreviousBatch = nullptr;
  std::vector<std::pair<nsDOMMutationObserver*, bool>> mObservers;  // BatchObserver
  std::vector<uint32_t> mRemovedNodes, mAddedNodes;
  uint32_t mBatchTarget = 0;
  bool mRemovalDone = false, mFromFirstToLast = false, mAllowNestedBatches = false;
  uint32_t mPrevSibling = 0, mNextSibling = 0;
};

void nsAutoMutationBatch::Done() {
  auto& mo = tJs->mo;
  if (mo.sCurrentBatch != this) return;
  mo.sCurrentBatch = mPreviousBatch;
  if (mObservers.empty()) {
    LeaveMutationHandling();
    return;
  }
  for (auto& [ob, wantsChildList] : mObservers) {
    std::vector<uint32_t> removedList;
    std::vector<nsMutationReceiver*> allObservers;
    GetAllSubtreeObserversFor(ob, mBatchTarget, allObservers);
    int32_t n = int32_t(mRemovedNodes.size());
    for (int32_t j = mFromFirstToLast ? 0 : n - 1; mFromFirstToLast ? j < n : j >= 0; mFromFirstToLast ? ++j : --j) {
      uint32_t removed = mRemovedNodes[j];
      if (wantsChildList) removedList.push_back(removed);
      for (nsMutationReceiver* r : allObservers) {
        nsMutationReceiver* orig = r->GetParent() ? r->GetParent() : r;
        if (GetReceiverFor(ob, removed, false) != orig) CreateTransientReceiver(ob, removed, orig);
      }
    }
    if (wantsChildList && (!mRemovedNodes.empty() || !mAddedNodes.empty())) {
      auto m = std::make_shared<nsDOMMutationRecord>();
      m->mType = "childList";
      m->mTarget = mBatchTarget;
      m->mRemovedNodes = std::move(removedList);
      m->mAddedNodes = mAddedNodes;
      m->mPreviousSibling = mPrevSibling;
      m->mNextSibling = mNextSibling;
      ob->mPendingMutations.push_back(std::move(m));
    }
    // always scheduled, so that the transient receivers are removed after the microtask
    ScheduleForRun(ob);
  }
  LeaveMutationHandling();
}

// nsMutationReceiverBase::ObservesAttr (no namespaces; nothing here is chrome-only)
static bool ObservesAttr(nsMutationReceiver* r, uint32_t aRegisterTarget, uint32_t aElement, const std::string& aAttr) {
  if (r->GetParent()) return ObservesAttr(r->GetParent(), aRegisterTarget, aElement, aAttr);
  if (!r->Attributes() || (!r->Subtree() && aElement != r->Target()) ||
      (r->Subtree() && SubtreeRoot(aRegisterTarget) != SubtreeRoot(aElement))) {
    return false;
  }
  if (r->AllAttributes()) return true;
  auto& filters = r->AttributeFilter();
  return std::find(filters.begin(), filters.end(), aAttr) != filters.end();
}

// nsMutationReceiver::AttributeWillChange
static void Receiver_AttributeWillChange(nsMutationReceiver* r, uint32_t aElement, const std::string& aAttribute) {
  if (nsAutoMutationBatch::IsBatching() || !ObservesAttr(r, r->RegisterTarget(), aElement, aAttribute)) return;
  nsDOMMutationRecord* m = CurrentRecord(r->Observer(), "attributes");
  if (!m->mTarget) {
    m->mTarget = aElement;
    m->mAttrName = aAttribute;
  }
  if (r->AttributeOldValue() && !m->mPrevValue) {
    const char* s;
    size_t len;
    if (mrt_dom_get_attr(tJs->host, aElement, aAttribute.data(), aAttribute.size(), &s, &len)) m->mPrevValue = std::string(s, len);
  }
}

// nsMutationReceiver::CharacterDataWillChange
static void Receiver_CharacterDataWillChange(nsMutationReceiver* r, uint32_t aContent) {
  if (nsAutoMutationBatch::IsBatching() || !r->CharacterData() || (!r->Subtree() && aContent != r->Target()) ||
      (r->Subtree() && SubtreeRoot(r->RegisterTarget()) != SubtreeRoot(aContent))) {
    return;
  }
  nsDOMMutationRecord* m = CurrentRecord(r->Observer(), "characterData");
  if (!m->mTarget) m->mTarget = aContent;
  if (r->CharacterDataOldValue() && !m->mPrevValue) {
    const char* s;
    size_t len;
    mrt_dom_text_content(tJs->host, aContent, &s, &len);
    m->mPrevValue = std::string(s, len);
  }
}

// nsMutationReceiver::ContentAppended
static void Receiver_ContentAppended(nsMutationReceiver* r, uint32_t aFirstNewContent) {
  uint32_t parent = ParentNode(aFirstNewContent);
  bool wantsChildList = r->ChildList() && ((r->Subtree() && SubtreeRoot(r->RegisterTarget()) == SubtreeRoot(parent)) ||
                                           parent == r->Target());
  if (!wantsChildList) return;
  if (nsAutoMutationBatch::IsBatching()) {
    if (parent == nsAutoMutationBatch::GetBatchTarget()) nsAutoMutationBatch::UpdateObserver(r->Observer(), wantsChildList);
    return;
  }
  nsDOMMutationRecord* m = CurrentRecord(r->Observer(), "childList");
  if (m->mTarget) return;  // already handled
  m->mTarget = parent;
  for (uint32_t n = aFirstNewContent; n; n = NextSibling(n)) m->mAddedNodes.push_back(n);
  m->mPreviousSibling = PreviousSibling(aFirstNewContent);
}

// nsMutationReceiver::ContentInserted
static void Receiver_ContentInserted(nsMutationReceiver* r, uint32_t aChild) {
  uint32_t parent = ParentNode(aChild);
  bool wantsChildList = r->ChildList() && ((r->Subtree() && SubtreeRoot(r->RegisterTarget()) == SubtreeRoot(parent)) ||
                                           parent == r->Target());
  if (!wantsChildList) return;
  if (nsAutoMutationBatch::IsBatching()) {
    if (parent == nsAutoMutationBatch::GetBatchTarget()) nsAutoMutationBatch::UpdateObserver(r->Observer(), wantsChildList);
    return;
  }
  nsDOMMutationRecord* m = CurrentRecord(r->Observer(), "childList");
  if (m->mTarget) return;  // already handled
  m->mTarget = parent;
  m->mAddedNodes.push_back(aChild);
  m->mPreviousSibling = PreviousSibling(aChild);
  m->mNextSibling = NextSibling(aChild);
}

// nsMutationReceiver::ContentWillBeRemoved
static void Receiver_ContentWillBeRemoved(nsMutationReceiver* r, uint32_t aChild) {
  uint32_t parent = ParentNode(aChild);
  if (r->Subtree() && SubtreeRoot(parent) != SubtreeRoot(r->RegisterTarget())) return;
  if (nsAutoMutationBatch::IsBatching()) {
    // for example the HTML parser moving nodes in its context node
    if (nsAutoMutationBatch::IsRemovalDone()) return;
    if (nsAutoMutationBatch::GetBatchTarget() != parent) return;
    bool wantsChildList = r->ChildList() && (r->Subtree() || parent == r->Target());
    if (wantsChildList || r->Subtree()) {
      nsAutoMutationBatch::NodeRemoved(aChild);
      nsAutoMutationBatch::UpdateObserver(r->Observer(), wantsChildList);
    }
    return;
  }
  nsDOMMutationObserver* o = r->Observer();
  if (r->Subtree()) {
    // no transient receiver when the node already has one observing the same set of nodes
    nsMutationReceiver* orig = r->GetParent() ? r->GetParent() : r;
    if (GetReceiverFor(o, aChild, false) != orig) {
      bool transientExists = false;
      if (auto* transient = TransientReceiversOf(o, aChild)) {
        for (auto& t : *transient) {
          if (t->GetParent() == orig) transientExists = true;
        }
      }
      if (!transientExists) CreateTransientReceiver(o, aChild, orig);
    }
  }
  if (r->ChildList() && (r->Subtree() || parent == r->Target())) {
    nsDOMMutationRecord* m = CurrentRecord(o, "childList");
    if (m->mTarget) return;  // already handled
    m->mTarget = parent;
    m->mRemovedNodes.push_back(aChild);
    m->mPreviousSibling = PreviousSibling(aChild);
    m->mNextSibling = NextSibling(aChild);
  }
  // always scheduled, so that mTransientReceivers is cleared after the microtask
  ScheduleForRun(o);
}

// MutationObservers::Notify and ForEachAncestorObserver: the receivers on the node and on each of
// its ancestors, crossing from a shadow root to its host, inside one mutation level
// (nsDOMMutationEnterLeave). Nothing to do until a page has observed a node.
template <typename F>
static void NotifyMutationObservers(uint32_t aNode, F&& aNotify) {
  if (!tJs->mo.mMayHaveDOMMutationObservers) return;
  EnterMutationHandling();
  uint32_t node = aNode;
  while (node) {
    auto it = tJs->mo.mNodeObservers.find(node);
    if (it != tJs->mo.mNodeObservers.end()) {
      // a copy: a notification can add a transient receiver to a list
      std::vector<nsMutationReceiver*> observers = it->second;
      for (nsMutationReceiver* r : observers) aNotify(r);
    }
    uint32_t last = node;
    node = ParentNode(node);
    if (!node && mrt_dom_kind(tJs->host, last) == KIND_SHADOW_ROOT) node = mrt_dom_link(tJs->host, last, LINK_HOST);
  }
  LeaveMutationHandling();
}

static void NotifyAttributeWillChange(uint32_t aElement, const std::string& aAttribute) {
  NotifyMutationObservers(aElement, [&](nsMutationReceiver* r) { Receiver_AttributeWillChange(r, aElement, aAttribute); });
}
static void NotifyCharacterDataWillChange(uint32_t aContent) {
  NotifyMutationObservers(aContent, [&](nsMutationReceiver* r) { Receiver_CharacterDataWillChange(r, aContent); });
}
static void NotifyContentAppended(uint32_t aContainer, uint32_t aFirstNewContent) {
  NotifyMutationObservers(aContainer, [&](nsMutationReceiver* r) { Receiver_ContentAppended(r, aFirstNewContent); });
}
static void NotifyContentInserted(uint32_t aContainer, uint32_t aChild) {
  NotifyMutationObservers(aContainer, [&](nsMutationReceiver* r) { Receiver_ContentInserted(r, aChild); });
}
static void NotifyContentWillBeRemoved(uint32_t aContainer, uint32_t aChild) {
  NotifyMutationObservers(aContainer, [&](nsMutationReceiver* r) { Receiver_ContentWillBeRemoved(r, aChild); });
}

// nsINode::InsertChildBefore's notification: children of a document and insertions before a node
// are ContentInserted, appends ContentAppended
static void NotifyChildAdded(uint32_t aParent, uint32_t aChild, bool aAppending) {
  if (aAppending && mrt_dom_kind(tJs->host, aParent) != KIND_DOCUMENT) NotifyContentAppended(aParent, aChild);
  else NotifyContentInserted(aParent, aChild);
}

static void FramesInserted(JSContext* cx, uint32_t aRoot);
static std::vector<size_t> FramesIn(uint32_t aChild);
static void CloseWindow(size_t aIndex);

// nsINode::RemoveChildNode with notification; the windows of the frames removed are closed
// (nsFrameLoader::Destroy from the frame element's UnbindFromTree)
static bool RemoveChildNode(uint32_t aParent, uint32_t aChild) {
  NotifyContentWillBeRemoved(aParent, aChild);
  std::vector<size_t> frames = tJs->windows.empty() ? std::vector<size_t>() : FramesIn(aChild);
  bool removed = mrt_dom_remove(tJs->host, aParent, aChild);
  for (size_t i : frames) CloseWindow(i);
  return removed;
}

static bool MakeMutationRecordObject(JSContext* cx, const nsDOMMutationRecord& aRecord, JS::MutableHandleValue aResult);

// nsDOMMutationObserver::HandleMutation: the callback with the observer's records
static void HandleMutation(JSContext* cx, nsDOMMutationObserver* o) {
  o->mWaitingForRun = false;
  for (auto& r : o->mReceivers) RemoveClones(r.get());
  for (auto& [node, list] : o->mTransientReceivers) {
    for (auto& t : list) {
      if (t->mRegisterTarget) RemoveMutationObserver(t.get());
    }
  }
  o->mTransientReceivers.clear();
  if (o->mPendingMutations.empty()) return;
  std::vector<std::shared_ptr<nsDOMMutationRecord>> pending = std::move(o->mPendingMutations);
  o->mPendingMutations.clear();
  JS::RootedObject records(cx, JS::NewArrayObject(cx, 0));
  if (!records) {
    JS_ClearPendingException(cx);
    return;
  }
  JS::RootedValue rec(cx);
  for (size_t i = 0; i < pending.size(); i++) {
    if (!MakeMutationRecordObject(cx, *pending[i], &rec) || !JS_SetElement(cx, records, uint32_t(i), rec)) {
      ReportCallbackException(cx, "MutationObserver callback");
      return;
    }
  }
  JS::RootedObject observer(cx, o->mReflector);
  JS::RootedValue callback(cx, JS::ObjectValue(*o->mCallback));
  JS::RootedValueArray<2> argv(cx);
  argv[0].setObject(*records);
  argv[1].setObject(*observer);
  JS::RootedValue rval(cx), thisv(cx, JS::ObjectValue(*observer));
  if (!JS::Call(cx, thisv, callback, argv, &rval)) ReportCallbackException(cx, "MutationObserver callback");
}

// Fires `slotchange` (bubbles, not composed) at each slot of the signal slot list, in signal order.
static void FireSlotChangeEvents(JSContext* cx, const std::vector<uint32_t>& aSlots) {
  JS::RootedObject global(cx, JS::CurrentGlobalOrNull(cx));
  JS::RootedValue ctor(cx), evt(cx), target(cx), rval(cx);
  for (uint32_t slot : aSlots) {
    if (!JS_GetProperty(cx, global, "Event", &ctor)) return ReportCallbackException(cx, "slotchange event");
    JS::RootedObject initObj(cx, JS_NewPlainObject(cx));
    if (!initObj || !JS_DefineProperty(cx, initObj, "bubbles", JS::TrueHandleValue, JSPROP_ENUMERATE)) {
      return ReportCallbackException(cx, "slotchange event");
    }
    JS::RootedValueArray<2> ctorArgs(cx);
    JS::RootedString type(cx, JS_NewStringCopyZ(cx, "slotchange"));
    if (!type) return ReportCallbackException(cx, "slotchange event");
    ctorArgs[0].setString(type);
    ctorArgs[1].setObject(*initObj);
    JS::RootedObject evtObj(cx);
    if (!JS::Construct(cx, ctor, ctorArgs, &evtObj)) return ReportCallbackException(cx, "slotchange event");
    evt.setObject(*evtObj);
    if (!Wrap(cx, slot, &target)) return ReportCallbackException(cx, "slotchange event");
    JS::RootedObject targetObj(cx, &target.toObject());
    if (!JS_CallFunctionName(cx, targetObj, "dispatchEvent", JS::HandleValueArray(evt), &rval)) {
      ReportCustomElementException(cx, "slotchange listener");
    }
  }
}

// nsDOMMutationObserver::HandleMutationsInternal: the signal slot list is taken first, the
// observers' callbacks run in creation order, then the slots get their slotchange events.
static bool MutationObserverMicroTask_Run(JSContext* cx, unsigned argc, JS::Value* vp) {
  JS::CallArgs args = JS::CallArgsFromVp(argc, vp);
  args.rval().setUndefined();
  auto& mo = tJs->mo;
  std::vector<uint32_t> signalList;
  if (mo.sPendingSignalSlots) {
    signalList = std::move(tJs->signalSlots);
    tJs->signalSlots.clear();
    mo.sPendingSignalSlots = false;
  }
  if (mo.sScheduledMutationObservers) {
    std::vector<nsDOMMutationObserver*> observers = std::move(*mo.sScheduledMutationObservers);
    mo.sScheduledMutationObservers.reset();
    for (nsDOMMutationObserver* o : observers) HandleMutation(cx, o);
  }
  FireSlotChangeEvents(cx, signalList);
  return true;
}

// DocGroup::SignalSlotChange (HTMLSlotElement::EnqueueSlotChangeEvent: once per slot until fired)
static void SignalSlotChange(JSContext* cx, uint32_t aSlot) {
  std::vector<uint32_t>& list = tJs->signalSlots;
  if (std::find(list.begin(), list.end(), aSlot) != list.end()) return;
  list.push_back(aSlot);
  if (!tJs->mo.sPendingSignalSlots) {
    QueueMutationObserverMicroTask(cx);
    tJs->mo.sPendingSignalSlots = true;
  }
}

// nsINode::ReplaceOrInsertBefore (not replacing): 0, or the DOMException code. The pre-insertion
// checks come first; a node is taken out of its old parent in a batch of its own; a fragment's
// children are taken out of it (back to front) and inserted in one batch on the parent.
static int32_t InsertInternal(JSContext* cx, uint32_t parent, uint32_t child, uint32_t before) {
  AutoCEReaction ceReaction(cx);
  if (int32_t r = mrt_dom_check_insert(tJs->host, parent, child, before)) return r;
  SlotChangeWatch slotWatch;
  slotWatch.WatchChildren(parent, child);
  uint32_t oldParent = ParentNode(child);
  if (oldParent) slotWatch.WatchChildren(oldParent, child);
  // inserting a node before itself inserts it before its next sibling
  uint32_t nodeToInsertBefore = before == child ? NextSibling(child) : before;
  bool isFragment = mrt_dom_kind(tJs->host, child) == KIND_FRAGMENT;
  std::vector<uint32_t> fragChildren;
  if (isFragment) {
    for (uint32_t c = FirstChild(child); c; c = NextSibling(c)) fragChildren.push_back(c);
  }
  // custom element reactions: what gets inserted, and whether it is moved out of the document first
  bool track = !tJs->ce.mDefinitions.empty();
  bool wasConnected = track && !isFragment && IsConnected(child);

  if (oldParent) {
    nsAutoMutationBatch mb(oldParent, true, true);
    uint32_t previous = PreviousSibling(child), next = NextSibling(child);
    RemoveChildNode(oldParent, child);
    if (nsAutoMutationBatch::GetCurrentBatch() == &mb) {
      mb.RemovalDone();
      mb.SetPrevSibling(previous);
      mb.SetNextSibling(next);
    }
  }

  if (isFragment) {
    {
      nsAutoMutationBatch mb(child, false, true);
      for (auto it = fragChildren.rbegin(); it != fragChildren.rend(); ++it) RemoveChildNode(child, *it);
    }
    nsAutoMutationBatch mb;
    nsAutoMutationBatch* mutationBatch = nsAutoMutationBatch::GetCurrentBatch();
    if (mutationBatch && mutationBatch != &mb) {
      mutationBatch = nullptr;
    } else {
      mb.Init(parent, true, true);
      mutationBatch = nsAutoMutationBatch::GetCurrentBatch();
    }
    if (mutationBatch) {
      mutationBatch->RemovalDone();
      mutationBatch->SetPrevSibling(nodeToInsertBefore ? PreviousSibling(nodeToInsertBefore) : LastChild(parent));
      mutationBatch->SetNextSibling(nodeToInsertBefore);
    }
    if (!fragChildren.empty()) {
      bool appending = mrt_dom_kind(tJs->host, parent) != KIND_DOCUMENT && !nodeToInsertBefore;
      for (uint32_t c : fragChildren) {
        mrt_dom_insert(tJs->host, parent, c, nodeToInsertBefore);
        if (!appending) NotifyContentInserted(parent, c);
      }
      if (mutationBatch && !appending) mutationBatch->NodesAdded();
      if (appending) {
        NotifyContentAppended(parent, fragChildren[0]);
        if (mutationBatch) mutationBatch->NodesAdded();
      }
    }
  } else {
    mrt_dom_insert(tJs->host, parent, child, nodeToInsertBefore);
    NotifyChildAdded(parent, child, !nodeToInsertBefore);
  }
  slotWatch.Finish(cx);
  if (track) {
    if (wasConnected) EnqueueDisconnectedReactions(child);
    if (IsConnected(parent)) {
      if (isFragment) {
        for (uint32_t c : fragChildren) EnqueueConnectedReactions(c);
      } else {
        EnqueueConnectedReactions(child);
      }
    }
  }
  // the frames inserted get their windows (nsGenericHTMLFrameElement::BindToTree)
  if (isFragment) {
    for (uint32_t c : fragChildren) FramesInserted(cx, c);
  } else {
    FramesInserted(cx, child);
  }
  return 0;
}

static bool Insert(JSContext* cx, JS::CallArgs& args, uint32_t parent, uint32_t child, uint32_t before) {
  if (int32_t r = InsertInternal(cx, parent, child, before)) return ThrowDOMException(cx, r);
  args.rval().set(args.get(0));
  return true;
}

// nsINode::RemoveChild after its checks: the removal with its slot, custom element and mutation
// observer notifications.
static void RemoveChildInternal(JSContext* cx, uint32_t parent, uint32_t child) {
  bool wasConnected = !tJs->ce.mDefinitions.empty() && IsConnected(child);
  SlotChangeWatch slotWatch;
  slotWatch.WatchChildren(parent, child);
  RemoveChildNode(parent, child);
  slotWatch.Finish(cx);
  if (wasConnected) EnqueueDisconnectedReactions(child);
}

// Node.textContent's setter: a text or comment node's data (CharacterData::SetData), or an element's
// or fragment's children replaced by one text node in a batch (nsContentUtils::SetNodeTextContent,
// without the text node reuse it skips when the document has mutation observers).
static void SetTextContent(JSContext* cx, uint32_t aNode, const std::string& aValue) {
  int kind = mrt_dom_kind(tJs->host, aNode);
  if (kind == KIND_TEXT || kind == KIND_COMMENT) {
    NotifyCharacterDataWillChange(aNode);
    mrt_dom_set_text_content(tJs->host, aNode, aValue.data(), aValue.size());
    return;
  }
  if (kind != KIND_ELEMENT && kind != KIND_FRAGMENT && kind != KIND_SHADOW_ROOT) return;
  nsAutoMutationBatch mb(aNode, true, false);
  while (uint32_t first = FirstChild(aNode)) RemoveChildInternal(cx, aNode, first);
  mb.RemovalDone();
  if (aValue.empty()) return;
  uint32_t text = mrt_dom_create_text(tJs->host, aValue.data(), aValue.size());
  mrt_dom_insert(tJs->host, aNode, text, 0);
  NotifyChildAdded(aNode, text, true);
  mb.NodesAdded();
}

// __mrtSetInnerHTML(target, fragment): innerHTML's setter after parsing (FragmentOrElement::
// SetInnerHTMLInternal): the children removed and the fragment's appended in one batch, which
// takes no nested batches, so it makes one childList record. `fragment` may be null.
static bool Global_mrtSetInnerHTML(JSContext* cx, unsigned argc, JS::Value* vp) {
  JS::CallArgs args = JS::CallArgsFromVp(argc, vp);
  AutoCEReaction ceReaction(cx);
  uint32_t target, fragment = 0;
  if (!ArgNode(cx, args, 0, &target) || !ArgNode(cx, args, 1, &fragment, true)) return false;
  nsAutoMutationBatch mb(target, true, false);
  while (uint32_t first = FirstChild(target)) RemoveChildInternal(cx, target, first);
  mb.RemovalDone();
  if (fragment) {
    if (int32_t r = InsertInternal(cx, target, fragment, 0)) return ThrowDOMException(cx, r);
  }
  mb.NodesAdded();
  args.rval().setUndefined();
  return true;
}

// __mrtReplaceChildren(target, node): ParentNode.replaceChildren after the nodes and strings are
// made into one node (nsINode::ReplaceChildren): the children removed and `node` appended in a
// batch that allows nested ones. `node` may be null.
static bool Global_mrtReplaceChildren(JSContext* cx, unsigned argc, JS::Value* vp) {
  JS::CallArgs args = JS::CallArgsFromVp(argc, vp);
  AutoCEReaction ceReaction(cx);
  uint32_t target, node = 0;
  if (!ArgNode(cx, args, 0, &target) || !ArgNode(cx, args, 1, &node, true)) return false;
  if (node) {
    if (int32_t r = mrt_dom_check_insert(tJs->host, target, node, 0)) return ThrowDOMException(cx, r);
  }
  nsAutoMutationBatch mb(target, true, true);
  while (uint32_t first = FirstChild(target)) RemoveChildInternal(cx, target, first);
  mb.RemovalDone();
  if (node) {
    if (int32_t r = InsertInternal(cx, target, node, 0)) return ThrowDOMException(cx, r);
    mb.NodesAdded();
  }
  args.rval().setUndefined();
  return true;
}

// A TypeError with the message, as Gecko's ErrorResult::ThrowTypeError does for WebIDL bindings.
static bool ThrowTypeError(JSContext* cx, const char* aMessage) {
  JS::RootedValue file(cx), message(cx), errorValue(cx);
  if (!NewString(cx, "", 0, &file) || !NewString(cx, aMessage, strlen(aMessage), &message)) return false;
  JS::RootedString fileString(cx, file.toString()), messageString(cx, message.toString());
  if (JS::CreateError(cx, JSEXN_TYPEERR, nullptr, fileString, 0, JS::ColumnNumberOneOrigin(), nullptr, messageString,
                      JS::NothingHandleValue, &errorValue)) {
    JS_SetPendingException(cx, errorValue);
  }
  return false;
}

// ---- MutationObserver and MutationRecord (dom/webidl/MutationObserver.webidl) ----

// The observer's reflector holds it in slot 0 and traces its callback. The reflector is traced from
// TracePreserved while the observer observes or has records to deliver, and is a weak pointer
// otherwise: SweepCache frees the observer when it dies (Gecko frees it with its last reference).
static void MutationObserver_trace(JSTracer* trc, JSObject* obj) {
  auto* o = JS::GetMaybePtrFromReservedSlot<nsDOMMutationObserver>(obj, 0);
  if (o && o->mCallback.unbarrieredGet()) JS::TraceEdge(trc, &o->mCallback, "MutationObserver callback");
}
static const JSClassOps kMutationObserverOps = {nullptr, nullptr, nullptr, nullptr, nullptr,
                                                nullptr, nullptr, nullptr, nullptr, MutationObserver_trace};
static const JSClass kMutationObserverClass = {"MutationObserver", JSCLASS_HAS_RESERVED_SLOTS(1), &kMutationObserverOps};

static bool MutationObserverHeld(nsDOMMutationObserver* o) { return o->Held(); }

// nsDOMMutationObserver::Constructor
static bool MutationObserver_constructor(JSContext* cx, unsigned argc, JS::Value* vp) {
  JS::CallArgs args = JS::CallArgsFromVp(argc, vp);
  if (!args.isConstructing()) {
    return ThrowTypeError(cx, "MutationObserver constructor: 'new' is required");
  }
  if (!args.get(0).isObject()) {
    return ThrowTypeError(cx, "MutationObserver constructor: Argument 1 is not an object.");
  }
  if (!JS::IsCallable(&args[0].toObject())) {
    return ThrowTypeError(cx, "MutationObserver constructor: Argument 1 is not callable.");
  }
  JS::RootedObject obj(cx, JS_NewObjectForConstructor(cx, &kMutationObserverClass, args));
  if (!obj) return false;
  auto o = std::make_unique<nsDOMMutationObserver>();
  o->mReflector = obj;
  o->mCallback = &args[0].toObject();
  o->mId = ++tJs->mo.sCount;
  JS::SetReservedSlot(obj, 0, JS::PrivateValue(o.get()));
  tJs->mo.mObservers.push_back(std::move(o));
  args.rval().setObject(*obj);
  return true;
}

static bool ThisObserver(JSContext* cx, const JS::CallArgs& args, const char* aMethod, nsDOMMutationObserver** aOut) {
  JSObject* obj = args.thisv().isObject() ? &args.thisv().toObject() : nullptr;
  *aOut = obj && JS::GetClass(obj) == &kMutationObserverClass ? JS::GetMaybePtrFromReservedSlot<nsDOMMutationObserver>(obj, 0) : nullptr;
  if (!*aOut) {
    std::string m = std::string("'") + aMethod + "' called on an object that does not implement interface MutationObserver.";
    return ThrowTypeError(cx, m.c_str());
  }
  return true;
}

// A dictionary member: false when it is undefined (not passed), with its value in *aValue
static bool DictMember(JSContext* cx, JS::HandleObject aDict, const char* aName, JS::MutableHandleValue aValue) {
  if (!aDict) {
    aValue.setUndefined();
    return true;
  }
  return JS_GetProperty(cx, aDict, aName, aValue);
}

// nsDOMMutationObserver::Observe (MutationObserverInit's members read in WebIDL's order)
static bool MutationObserver_observe(JSContext* cx, unsigned argc, JS::Value* vp) {
  JS::CallArgs args = JS::CallArgsFromVp(argc, vp);
  nsDOMMutationObserver* o;
  if (!ThisObserver(cx, args, "observe", &o)) return false;
  uint32_t target;
  if (!args.get(0).isObject()) {
    return ThrowTypeError(cx, "MutationObserver.observe: Argument 1 is not an object.");
  }
  if (!ArgNode(cx, args, 0, &target)) {
    JS_ClearPendingException(cx);
    return ThrowTypeError(cx, "MutationObserver.observe: Argument 1 does not implement interface Node.");
  }
  JS::RootedObject dict(cx);
  if (args.get(1).isObject()) {
    dict = &args[1].toObject();
  } else if (!args.get(1).isNullOrUndefined()) {
    return ThrowTypeError(cx, "MutationObserver.observe: Argument 2 can't be converted to a dictionary.");
  }
  JS::RootedValue v(cx);
  // attributeFilter, attributeOldValue, attributes, characterData, characterDataOldValue, childList, subtree
  std::optional<std::vector<std::string>> attributeFilter;
  if (!DictMember(cx, dict, "attributeFilter", &v)) return false;
  if (!v.isUndefined()) {
    bool isArray = false;
    if (!v.isObject() || !JS::IsArrayObject(cx, v, &isArray) || !isArray) {
      JS_ClearPendingException(cx);
      return ThrowTypeError(cx, "MutationObserver.observe: 'attributeFilter' member of MutationObserverInit can't be converted to a sequence.");
    }
    JS::RootedObject arr(cx, &v.toObject());
    uint32_t len;
    if (!JS::GetArrayLength(cx, arr, &len)) return false;
    attributeFilter.emplace();
    JS::RootedValue item(cx);
    for (uint32_t i = 0; i < len; i++) {
      std::string name;
      if (!JS_GetElement(cx, arr, i, &item) || !ArgUTF8(cx, item, name)) return false;
      attributeFilter->push_back(std::move(name));
    }
  }
  std::optional<bool> attributeOldValue, attributes, characterData, characterDataOldValue;
  bool childList = false, subtree = false;
  for (auto [name, member] : {std::pair{"attributeOldValue", &attributeOldValue}, std::pair{"attributes", &attributes},
                              std::pair{"characterData", &characterData}, std::pair{"characterDataOldValue", &characterDataOldValue}}) {
    if (!DictMember(cx, dict, name, &v)) return false;
    if (!v.isUndefined()) *member = JS::ToBoolean(v);
  }
  if (!DictMember(cx, dict, "childList", &v)) return false;
  childList = JS::ToBoolean(v);
  if (!DictMember(cx, dict, "subtree", &v)) return false;
  subtree = JS::ToBoolean(v);

  bool attrs = attributes.value_or(false);
  bool charData = characterData.value_or(false);
  bool attrOld = attributeOldValue.value_or(false);
  bool charDataOld = characterDataOldValue.value_or(false);
  if (!attributes && (attributeOldValue || attributeFilter)) attrs = true;
  if (!characterData && characterDataOldValue) charData = true;
  if (!(childList || attrs || charData)) {
    return ThrowTypeError(cx, "MutationObserver.observe: One of 'childList', 'attributes', 'characterData' must not be false.");
  }
  if (attributeOldValue && *attributeOldValue && !attrs) {
    return ThrowTypeError(cx, "MutationObserver.observe: If 'attributeOldValue' is true, 'attributes' must not be false.");
  }
  if (attributeFilter && !attrs) {
    return ThrowTypeError(cx, "MutationObserver.observe: If 'attributesFilter' is present, 'attributes' must not be false.");
  }
  if (characterDataOldValue && *characterDataOldValue && !charData) {
    return ThrowTypeError(cx, "MutationObserver.observe: If 'characterDataOldValue' is true, 'characterData' must not be false.");
  }
  nsMutationReceiver* r = GetReceiverFor(o, target, true);
  r->mChildList = childList;
  r->mAttributes = attrs;
  r->mCharacterData = charData;
  r->mSubtree = subtree;
  r->mAttributeOldValue = attrOld;
  r->mCharacterDataOldValue = charDataOld;
  r->mAttributeFilter = attributeFilter ? std::move(*attributeFilter) : std::vector<std::string>();
  r->mAllAttributes = !attributeFilter;
  RemoveClones(r);
  args.rval().setUndefined();
  return true;
}

// nsDOMMutationObserver::Disconnect
static bool MutationObserver_disconnect(JSContext* cx, unsigned argc, JS::Value* vp) {
  JS::CallArgs args = JS::CallArgsFromVp(argc, vp);
  nsDOMMutationObserver* o;
  if (!ThisObserver(cx, args, "disconnect", &o)) return false;
  for (auto& r : o->mReceivers) DisconnectReceiver(r.get());
  o->mReceivers.clear();
  o->mCurrentMutations.clear();
  o->mPendingMutations.clear();
  args.rval().setUndefined();
  return true;
}

// nsDOMMutationObserver::TakeRecords
static bool MutationObserver_takeRecords(JSContext* cx, unsigned argc, JS::Value* vp) {
  JS::CallArgs args = JS::CallArgsFromVp(argc, vp);
  nsDOMMutationObserver* o;
  if (!ThisObserver(cx, args, "takeRecords", &o)) return false;
  std::vector<std::shared_ptr<nsDOMMutationRecord>> pending = std::move(o->mPendingMutations);
  o->mPendingMutations.clear();
  JS::RootedObject records(cx, JS::NewArrayObject(cx, 0));
  if (!records) return false;
  JS::RootedValue rec(cx);
  for (size_t i = 0; i < pending.size(); i++) {
    if (!MakeMutationRecordObject(cx, *pending[i], &rec) || !JS_SetElement(cx, records, uint32_t(i), rec)) return false;
  }
  args.rval().setObject(*records);
  return true;
}

static const JSFunctionSpec kMutationObserverFns[] = {
    JS_FN("observe", MutationObserver_observe, 1, JSPROP_ENUMERATE),
    JS_FN("disconnect", MutationObserver_disconnect, 0, JSPROP_ENUMERATE),
    JS_FN("takeRecords", MutationObserver_takeRecords, 0, JSPROP_ENUMERATE), JS_FS_END};

// A MutationRecord: its values in reserved slots, read by the prototype's getters. addedNodes and
// removedNodes are one array each ([SameObject]), with NodeList's prototype when there is one.
enum MutationRecordSlot { MR_TYPE, MR_TARGET, MR_ADDED, MR_REMOVED, MR_PREVIOUS, MR_NEXT, MR_ATTR_NAME, MR_ATTR_NS, MR_OLD_VALUE, MR_SLOTS };
static const JSClass kMutationRecordClass = {"MutationRecord", JSCLASS_HAS_RESERVED_SLOTS(MR_SLOTS)};

static bool NodeOrNull(JSContext* cx, uint32_t aNode, JS::MutableHandleValue aOut) {
  if (!aNode) {
    aOut.setNull();
    return true;
  }
  return Wrap(cx, aNode, aOut);
}

static bool NodeListOf(JSContext* cx, const std::vector<uint32_t>& aNodes, JS::MutableHandleValue aOut) {
  JS::RootedObject arr(cx, JS::NewArrayObject(cx, aNodes.size()));
  if (!arr) return false;
  JS::RootedValue node(cx);
  for (size_t i = 0; i < aNodes.size(); i++) {
    if (!Wrap(cx, aNodes[i], &node) || !JS_SetElement(cx, arr, uint32_t(i), node)) return false;
  }
  JS::RootedObject global(cx, JS::CurrentGlobalOrNull(cx));
  JS::RootedValue ctor(cx), proto(cx);
  bool found = false;
  if (JS_HasProperty(cx, global, "NodeList", &found) && found && JS_GetProperty(cx, global, "NodeList", &ctor) && ctor.isObject()) {
    JS::RootedObject ctorObj(cx, &ctor.toObject());
    if (JS_GetProperty(cx, ctorObj, "prototype", &proto) && proto.isObject()) {
      JS::RootedObject protoObj(cx, &proto.toObject());
      if (!JS_SetPrototype(cx, arr, protoObj)) return false;
    }
  }
  JS_ClearPendingException(cx);
  aOut.setObject(*arr);
  return true;
}

static bool MakeMutationRecordObject(JSContext* cx, const nsDOMMutationRecord& r, JS::MutableHandleValue aResult) {
  JS::RootedObject global(cx, JS::CurrentGlobalOrNull(cx));
  JS::RootedObject proto(cx, &JS::GetReservedSlot(global, SLOT_MUTATION_RECORD).toObject());
  JS::RootedObject obj(cx, JS_NewObjectWithGivenProto(cx, &kMutationRecordClass, proto));
  if (!obj) return false;
  JS::RootedValue v(cx);
  if (!NewString(cx, r.mType, strlen(r.mType), &v)) return false;
  JS::SetReservedSlot(obj, MR_TYPE, v);
  if (!NodeOrNull(cx, r.mTarget, &v)) return false;
  JS::SetReservedSlot(obj, MR_TARGET, v);
  if (!NodeListOf(cx, r.mAddedNodes, &v)) return false;
  JS::SetReservedSlot(obj, MR_ADDED, v);
  if (!NodeListOf(cx, r.mRemovedNodes, &v)) return false;
  JS::SetReservedSlot(obj, MR_REMOVED, v);
  if (!NodeOrNull(cx, r.mPreviousSibling, &v)) return false;
  JS::SetReservedSlot(obj, MR_PREVIOUS, v);
  if (!NodeOrNull(cx, r.mNextSibling, &v)) return false;
  JS::SetReservedSlot(obj, MR_NEXT, v);
  v.setNull();
  if (r.mAttrName && !NewString(cx, r.mAttrName->data(), r.mAttrName->size(), &v)) return false;
  JS::SetReservedSlot(obj, MR_ATTR_NAME, v);
  JS::SetReservedSlot(obj, MR_ATTR_NS, JS::NullValue());
  v.setNull();
  if (r.mPrevValue && !NewString(cx, r.mPrevValue->data(), r.mPrevValue->size(), &v)) return false;
  JS::SetReservedSlot(obj, MR_OLD_VALUE, v);
  aResult.setObject(*obj);
  return true;
}

template <int Slot>
static bool MutationRecord_get(JSContext* cx, unsigned argc, JS::Value* vp) {
  JS::CallArgs args = JS::CallArgsFromVp(argc, vp);
  JSObject* obj = args.thisv().isObject() ? &args.thisv().toObject() : nullptr;
  if (!obj || JS::GetClass(obj) != &kMutationRecordClass) {
    return ThrowTypeError(cx, "'get' called on an object that does not implement interface MutationRecord.");
  }
  args.rval().set(JS::GetReservedSlot(obj, Slot));
  return true;
}

static const JSPropertySpec kMutationRecordProps[] = {
    JS_PSG("type", MutationRecord_get<MR_TYPE>, JSPROP_ENUMERATE),
    JS_PSG("target", MutationRecord_get<MR_TARGET>, JSPROP_ENUMERATE),
    JS_PSG("addedNodes", MutationRecord_get<MR_ADDED>, JSPROP_ENUMERATE),
    JS_PSG("removedNodes", MutationRecord_get<MR_REMOVED>, JSPROP_ENUMERATE),
    JS_PSG("previousSibling", MutationRecord_get<MR_PREVIOUS>, JSPROP_ENUMERATE),
    JS_PSG("nextSibling", MutationRecord_get<MR_NEXT>, JSPROP_ENUMERATE),
    JS_PSG("attributeName", MutationRecord_get<MR_ATTR_NAME>, JSPROP_ENUMERATE),
    JS_PSG("attributeNamespace", MutationRecord_get<MR_ATTR_NS>, JSPROP_ENUMERATE),
    JS_PSG("oldValue", MutationRecord_get<MR_OLD_VALUE>, JSPROP_ENUMERATE), JS_PS_END};

NATIVE(Node_appendChild) {
  THIS_NODE;
  uint32_t child;
  if (!ArgNode(cx, args, 0, &child)) return false;
  return Insert(cx, args, self, child, 0);
}
NATIVE(Node_insertBefore) {
  THIS_NODE;
  uint32_t child, before;
  if (!ArgNode(cx, args, 0, &child) || !ArgNode(cx, args, 1, &before, true)) return false;
  return Insert(cx, args, self, child, before);
}
NATIVE(Node_removeChild) {
  THIS_NODE;
  AutoCEReaction ceReaction(cx);
  uint32_t child;
  if (!ArgNode(cx, args, 0, &child)) return false;
  if (ParentNode(child) != self) return ThrowDOMException(cx, 2);
  RemoveChildInternal(cx, self, child);
  args.rval().set(args.get(0));
  return true;
}
NATIVE(Node_hasChildNodes) {
  THIS_NODE;
  args.rval().setBoolean(mrt_dom_link(tJs->host, self, 1) != 0);
  return true;
}
NATIVE(Node_contains) {
  THIS_NODE;
  uint32_t other = NodeOf(args.get(0));
  while (other && other != self) other = mrt_dom_link(tJs->host, other, 0);
  args.rval().setBoolean(other != 0);
  return true;
}

// ---- Element ----

NATIVE(Element_tagName) {
  THIS_NODE;
  return NodeName(cx, args, self);
}
// Element.localName: the name as stored, case kept (an SVG element's clipPath)
NATIVE(Element_localName) {
  THIS_NODE;
  std::string name = LocalName(self);
  return NewString(cx, name.data(), name.size(), args.rval());
}
// namespaceURI, from the arena node's namespace (the parser's numbers: nsNameSpaceManager.h in the
// shim). createElementNS keeps the parser's namespaces only (GAPS B18).
NATIVE(Element_namespaceURI) {
  THIS_NODE;
  const char* name;
  size_t len;
  int32_t ns = mrt_dom_name(tJs->host, self, &name, &len);
  const char* uri = ns == 3 ? "http://www.w3.org/1999/xhtml" : ns == 6 ? "http://www.w3.org/2000/svg"
                  : ns == 5 ? "http://www.w3.org/1998/Math/MathML" : nullptr;
  if (!uri) {
    args.rval().setNull();
    return true;
  }
  return NewString(cx, uri, strlen(uri), args.rval());
}

static bool GetAttr(JSContext* cx, JS::CallArgs& args, uint32_t self, const std::string& name, bool null_if_missing) {
  const char* s;
  size_t len;
  if (!mrt_dom_get_attr(tJs->host, self, name.data(), name.size(), &s, &len)) {
    if (null_if_missing) {
      args.rval().setNull();
      return true;
    }
    s = "";
    len = 0;
  }
  return NewString(cx, s, len, args.rval());
}

static void LowerASCII(std::string& s) {
  for (char& c : s) {
    if (c >= 'A' && c <= 'Z') c += 32;
  }
}
// The name getAttribute and the other qualified-name methods look up: lowercased only for an HTML
// element in an HTML document (Element::GetAttribute, SetAttribute, RemoveAttribute); an SVG
// element's `viewBox` keeps its case. Every document here is an HTML document.
static void AttrNameForElement(uint32_t self, std::string& name) {
  const char* tag;
  size_t len;
  if (mrt_dom_name(tJs->host, self, &tag, &len) == 3) LowerASCII(name);
}

NATIVE(Element_getAttribute) {
  THIS_NODE;
  std::string name;
  if (!ArgUTF8(cx, args.get(0), name)) return false;
  AttrNameForElement(self, name);
  return GetAttr(cx, args, self, name, true);
}
NATIVE(Element_hasAttribute) {
  THIS_NODE;
  std::string name;
  if (!ArgUTF8(cx, args.get(0), name)) return false;
  AttrNameForElement(self, name);
  const char* s;
  size_t len;
  args.rval().setBoolean(mrt_dom_get_attr(tJs->host, self, name.data(), name.size(), &s, &len));
  return true;
}
// The attribute's value before a change, for attributeChangedCallback.
static std::optional<std::string> OldAttr(uint32_t self, const std::string& name) {
  const char* s;
  size_t len;
  if (!mrt_dom_get_attr(tJs->host, self, name.data(), name.size(), &s, &len)) return std::nullopt;
  return std::string(s, len);
}

// The shadow trees a change of attribute `name` on `self` can reassign: the host's, for a host's
// child's `slot`; the slot's own, for a slot's `name`.
static void WatchAttrSlots(SlotChangeWatch& aWatch, uint32_t self, const std::string& name) {
  if (name == "slot") {
    if (uint32_t parent = mrt_dom_link(tJs->host, self, LINK_PARENT)) aWatch.Watch(mrt_dom_link(tJs->host, parent, LINK_SHADOW_ROOT));
  } else if (name == "name" && IsSlot(self)) {
    aWatch.Watch(ContainingShadow(self));
  }
}

static void FrameAttributeChanged(uint32_t aElement, const std::string& aName);
static void ImageSrcChanged(uint32_t aElement);

static void SetAttr(uint32_t self, const std::string& name, const std::string& value) {
  std::optional<std::string> old = OldAttr(self, name);
  SlotChangeWatch slotWatch;
  WatchAttrSlots(slotWatch, self, name);
  NotifyAttributeWillChange(self, name);
  mrt_dom_set_attr(tJs->host, self, name.data(), name.size(), value.data(), value.size());
  slotWatch.Finish(tJs->cx);
  EnqueueAttributeChanged(self, name, std::move(old), value);
  if ((name == "src" || name == "srcdoc" || name == "name") && !tJs->windows.empty()) FrameAttributeChanged(self, name);
  if (name == "src") ImageSrcChanged(self);
}

// Element::HasAttributes: whether the element has any attribute.
NATIVE(Element_hasAttributes) {
  THIS_NODE;
  const char* names;
  size_t len = 0;
  if (mrt_dom_kind(tJs->host, self) == KIND_ELEMENT) mrt_dom_attr_names(tJs->host, self, &names, &len);
  args.rval().setBoolean(len > 0);
  return true;
}
NATIVE(Element_setAttribute) {
  THIS_NODE;
  AutoCEReaction ceReaction(cx);
  std::string name, value;
  if (!ArgUTF8(cx, args.get(0), name) || !ArgUTF8(cx, args.get(1), value)) return false;
  AttrNameForElement(self, name);
  SetAttr(self, name, value);
  args.rval().setUndefined();
  return true;
}
NATIVE(Element_removeAttribute) {
  THIS_NODE;
  AutoCEReaction ceReaction(cx);
  std::string name;
  if (!ArgUTF8(cx, args.get(0), name)) return false;
  AttrNameForElement(self, name);
  std::optional<std::string> old = OldAttr(self, name);
  SlotChangeWatch slotWatch;
  WatchAttrSlots(slotWatch, self, name);
  // Element::UnsetAttr: nothing to notify when the element has no such attribute
  if (old) NotifyAttributeWillChange(self, name);
  mrt_dom_remove_attr(tJs->host, self, name.data(), name.size());
  slotWatch.Finish(cx);
  if (old) EnqueueAttributeChanged(self, name, std::move(old), std::nullopt);
  if (old && name == "src") ImageSrcChanged(self);
  if (old && name == "name" && !tJs->windows.empty()) FrameAttributeChanged(self, name);
  args.rval().setUndefined();
  return true;
}

// Reflected string attributes: `id` and `className` (the class attribute).
static bool Reflect(JSContext* cx, unsigned argc, JS::Value* vp, const char* attr, bool set) {
  THIS_NODE;
  std::string name(attr);
  if (!set) return GetAttr(cx, args, self, name, false);
  AutoCEReaction ceReaction(cx);
  std::string value;
  if (!ArgUTF8(cx, args.get(0), value)) return false;
  SetAttr(self, name, value);
  args.rval().setUndefined();
  return true;
}
NATIVE(Element_id) { return Reflect(cx, argc, vp, "id", false); }
NATIVE(Element_set_id) { return Reflect(cx, argc, vp, "id", true); }
NATIVE(Element_className) { return Reflect(cx, argc, vp, "class", false); }
NATIVE(Element_set_className) { return Reflect(cx, argc, vp, "class", true); }

// querySelector and querySelectorAll on elements and the document.
static bool Query(JSContext* cx, unsigned argc, JS::Value* vp, bool all) {
  THIS_NODE;
  std::string sel;
  if (!ArgUTF8(cx, args.get(0), sel)) return false;
  int32_t n = mrt_dom_query(tJs->host, self, sel.data(), sel.size(), all ? 0 : 1);
  if (n < 0) {
    JS_ReportErrorUTF8(cx, "SyntaxError: '%s' is not a valid selector", sel.c_str());
    return false;
  }
  if (!all) return Wrap(cx, n ? mrt_dom_result(tJs->host, 0) : 0, args.rval());
  // the ids first: wrapping can run a GC, but not script, so the result list stays as it is
  std::vector<uint32_t> ids(n);
  for (int32_t i = 0; i < n; i++) ids[i] = mrt_dom_result(tJs->host, i);
  JS::RootedObject arr(cx, JS::NewArrayObject(cx, 0));
  if (!arr) return false;
  JS::RootedValue v(cx);
  for (int32_t i = 0; i < n; i++) {
    if (!Wrap(cx, ids[i], &v) || !JS_SetElement(cx, arr, i, v)) return false;
  }
  args.rval().setObject(*arr);
  return true;
}
// Element.matches and Element.closest, by Stylo's selector matching on the one element (Gecko's
// Servo_SelectorList_Matches and Servo_SelectorList_Closest).
static bool MatchSelector(JSContext* cx, unsigned argc, JS::Value* vp, bool closest) {
  THIS_NODE;
  std::string sel;
  if (!ArgUTF8(cx, args.get(0), sel)) return false;
  int64_t r = mrt_dom_match(tJs->host, self, sel.data(), sel.size(), closest);
  if (r < 0) {
    JS_ReportErrorUTF8(cx, "SyntaxError: '%s' is not a valid selector", sel.c_str());
    return false;
  }
  if (!closest) {
    args.rval().setBoolean(r != 0);
    return true;
  }
  return Wrap(cx, uint32_t(r), args.rval());
}
NATIVE(Element_matches) { return MatchSelector(cx, argc, vp, false); }
NATIVE(Element_closest) { return MatchSelector(cx, argc, vp, true); }

NATIVE(ParentNode_querySelector) { return Query(cx, argc, vp, false); }
NATIVE(ParentNode_querySelectorAll) { return Query(cx, argc, vp, true); }

// Layout reads. Each one flushes style and layout on the Rust side. getBoundingClientRect: the border
// box in the document as a plain object.
static bool Box(JSContext* cx, unsigned argc, JS::Value* vp) {
  THIS_NODE;
  double b[4] = {0, 0, 0, 0};
  mrt_dom_box(tJs->host, self, b);
  JS::RootedObject r(cx, JS_NewPlainObject(cx));
  if (!r) return false;
  const char* names[] = {"x", "y", "width", "height", "left", "top", "right", "bottom"};
  double values[] = {b[0], b[1], b[2], b[3], b[0], b[1], b[0] + b[2], b[1] + b[3]};
  for (int i = 0; i < 8; i++) {
    JS::RootedValue v(cx, JS::DoubleValue(values[i]));
    if (!JS_DefineProperty(cx, r, names[i], v, JSPROP_ENUMERATE)) return false;
  }
  args.rval().setObject(*r);
  return true;
}
// __mrtBorderBox(element): its border box [x, y, width, height] in CSS px, or null when it has no
// frame (display: none, not connected): what IntersectionObserver needs to tell a zero-size box
// from no box (DOMIntersectionObserver::Intersect returns early without a primary frame).
NATIVE(Global_mrtBorderBox) {
  JS::CallArgs args = JS::CallArgsFromVp(argc, vp);
  uint32_t id;
  if (!ArgNode(cx, args, 0, &id)) return false;
  double b[4] = {0, 0, 0, 0};
  if (!mrt_dom_box(tJs->host, id, b)) {
    args.rval().setNull();
    return true;
  }
  JS::RootedValueArray<4> values(cx);
  for (int i = 0; i < 4; i++) values[i].setDouble(b[i]);
  JSObject* array = JS::NewArrayObject(cx, values);
  if (!array) return false;
  args.rval().setObject(*array);
  return true;
}
// __mrtClientScroll(element): [clientWidth, clientHeight, scrollWidth, scrollHeight] as Gecko's
// Element::ClientWidth and the others give them (AppUnitsToIntCSSPixels: rounded), from the client
// area and scroll size of the element's frame; zeros without a frame. The scrolling element is
// bridge.js's (the viewport).
NATIVE(Global_mrtClientScroll) {
  JS::CallArgs args = JS::CallArgsFromVp(argc, vp);
  uint32_t id;
  if (!ArgNode(cx, args, 0, &id)) return false;
  double b[4] = {0, 0, 0, 0};
  mrt_dom_client_scroll(tJs->host, id, b);
  JS::RootedValueArray<4> values(cx);
  for (int i = 0; i < 4; i++) values[i].setInt32(int32_t(b[i] + 0.5));
  JSObject* array = JS::NewArrayObject(cx, values);
  if (!array) return false;
  args.rval().setObject(*array);
  return true;
}
// __mrtRootScroll(x, y): the page document's root scroll offset in CSS px, which the next layout
// read flushes with (Host::set_root_scroll)
NATIVE(Global_mrtRootScroll) {
  JS::CallArgs args = JS::CallArgsFromVp(argc, vp);
  double x = 0, y = 0;
  if (!JS::ToNumber(cx, args.get(0), &x) || !JS::ToNumber(cx, args.get(1), &y)) return false;
  mrt_dom_set_root_scroll(tJs->host, x, y);
  args.rval().setUndefined();
  return true;
}
// __mrtElementScroll(element, x, y): an element scroll container's scroll offset in CSS px, which
// the next layout read flushes with (Host::set_element_scroll); 0, 0 when it is not scrolled
NATIVE(Global_mrtElementScroll) {
  JS::CallArgs args = JS::CallArgsFromVp(argc, vp);
  uint32_t id;
  if (!ArgNode(cx, args, 0, &id)) return false;
  double x = 0, y = 0;
  if (!JS::ToNumber(cx, args.get(1), &x) || !JS::ToNumber(cx, args.get(2), &y)) return false;
  mrt_dom_set_element_scroll(tJs->host, id, x, y);
  args.rval().setUndefined();
  return true;
}
// __mrtViewport(document): [width, height] of the document's viewport in CSS px, or null when it has
// none (a frame element without a frame): nsGlobalWindowOuter::GetInnerSize's visible area. A frame's
// is its frame element's content box: its parent's layout is flushed first.
NATIVE(Global_mrtViewport) {
  JS::CallArgs args = JS::CallArgsFromVp(argc, vp);
  uint32_t id;
  if (!ArgNode(cx, args, 0, &id)) return false;
  int32_t size[2] = {0, 0};
  if (!mrt_dom_viewport(tJs->host, id, size)) {
    args.rval().setNull();
    return true;
  }
  JS::RootedValueArray<2> values(cx);
  values[0].setInt32(size[0]);
  values[1].setInt32(size[1]);
  JSObject* array = JS::NewArrayObject(cx, values);
  if (!array) return false;
  args.rval().setObject(*array);
  return true;
}
// __mrtResizeBoxes(elements, flush): for each element, what a ResizeObserver reads of it: 11 numbers
// each in one flat array (has a frame, border-box inline and block size, content-box inline and block
// size, device-pixel content-box inline and block size, contentRect x, y, width, height; see
// PresShellFrames::ResizeObserverBoxes). With flush, its document's layout is flushed first; without,
// the frames are read as the last flush left them (ResizeObserver's broadcast flushes nothing).
NATIVE(Global_mrtResizeBoxes) {
  JS::CallArgs args = JS::CallArgsFromVp(argc, vp);
  if (!args.get(0).isObject()) {
    JS_ReportErrorUTF8(cx, "TypeError: __mrtResizeBoxes takes an array of elements");
    return false;
  }
  JS::RootedObject list(cx, &args[0].toObject());
  uint32_t n;
  if (!JS::GetArrayLength(cx, list, &n)) return false;
  std::vector<uint32_t> ids(n);
  for (uint32_t i = 0; i < n; i++) {
    JS::RootedValue item(cx);
    if (!JS_GetElement(cx, list, i, &item)) return false;
    ids[i] = NodeOf(item);
  }
  std::vector<double> out(size_t(n) * 11, 0.0);
  if (n) mrt_dom_resize_boxes(tJs->host, ids.data(), n, JS::ToBoolean(args.get(1)), out.data());
  JS::RootedValueVector values(cx);
  if (!values.reserve(out.size())) return false;
  for (double v : out) values.infallibleAppend(JS::DoubleValue(v));
  JSObject* array = JS::NewArrayObject(cx, values);
  if (!array) return false;
  args.rval().setObject(*array);
  return true;
}
// __mrtIntersections(owner, root, margins, targets): what an IntersectionObserver's update computes
// for each target (DOMIntersectionObserver::ComputeInput and Intersect), 14 numbers each in one flat
// array, in app units: computed (0: Intersect returned early), rootBounds x, y, width, height, the
// target's rectangle, intersects, the intersection rectangle. owner is the observer's document, root
// null for the implicit root or an element or document; margins the rootMargin then the scrollMargin
// (top, right, bottom, left, each a value and 1 for a percentage). Layout is flushed first.
NATIVE(Global_mrtIntersections) {
  JS::CallArgs args = JS::CallArgsFromVp(argc, vp);
  if (!args.get(2).isObject() || !args.get(3).isObject()) {
    JS_ReportErrorUTF8(cx, "TypeError: __mrtIntersections takes margins and an array of elements");
    return false;
  }
  uint32_t owner = NodeOf(args.get(0)), root = NodeOf(args.get(1));
  JS::RootedObject marginList(cx, &args[2].toObject()), list(cx, &args[3].toObject());
  double margins[16] = {0};
  for (uint32_t i = 0; i < 16; i++) {
    JS::RootedValue item(cx);
    if (!JS_GetElement(cx, marginList, i, &item)) return false;
    if (!JS::ToNumber(cx, item, &margins[i])) return false;
  }
  uint32_t n;
  if (!JS::GetArrayLength(cx, list, &n)) return false;
  std::vector<uint32_t> ids(n);
  for (uint32_t i = 0; i < n; i++) {
    JS::RootedValue item(cx);
    if (!JS_GetElement(cx, list, i, &item)) return false;
    ids[i] = NodeOf(item);
  }
  std::vector<double> out(size_t(n) * 14, 0.0);
  if (n) mrt_dom_intersections(tJs->host, owner, root, margins, ids.data(), n, out.data());
  JS::RootedValueVector values(cx);
  if (!values.reserve(out.size())) return false;
  for (double v : out) values.infallibleAppend(JS::DoubleValue(v));
  JSObject* array = JS::NewArrayObject(cx, values);
  if (!array) return false;
  args.rval().setObject(*array);
  return true;
}
// __mrtNodesFromPoint(root, x, y, multiple): the elements under (x, y), in CSS px floats, of root, a
// document or a shadow root, the topmost first: elementsFromPoint's list, or with multiple false
// elementFromPoint's element alone (DocumentOrShadowRoot's QueryNodesFromPoint; Host::nodes_from_point).
// Layout is flushed first.
NATIVE(Global_mrtNodesFromPoint) {
  JS::CallArgs args = JS::CallArgsFromVp(argc, vp);
  uint32_t root = NodeOf(args.get(0));
  double x, y;
  if (!JS::ToNumber(cx, args.get(1), &x) || !JS::ToNumber(cx, args.get(2), &y)) return false;
  int multiple = JS::ToBoolean(args.get(3)) ? 1 : 0;
  std::vector<uint32_t> ids(64);
  size_t n = root ? mrt_dom_nodes_from_point(tJs->host, root, float(x), float(y), multiple, ids.data(), ids.size()) : 0;
  if (n > ids.size()) {
    ids.resize(n);
    n = mrt_dom_nodes_from_point(tJs->host, root, float(x), float(y), multiple, ids.data(), ids.size());
    if (n > ids.size()) n = ids.size();
  }
  JS::RootedValueVector values(cx);
  if (!values.reserve(n)) return false;
  for (size_t i = 0; i < n; i++) {
    JS::RootedValue v(cx);
    if (!Wrap(cx, ids[i], &v)) return false;
    values.infallibleAppend(v);
  }
  JSObject* array = JS::NewArrayObject(cx, values);
  if (!array) return false;
  args.rval().setObject(*array);
  return true;
}
// __mrtInnerText(element): the element's innerText (nsGenericHTMLElement::GetInnerText;
// Host::inner_text). Layout is flushed first.
NATIVE(Global_mrtInnerText) {
  JS::CallArgs args = JS::CallArgsFromVp(argc, vp);
  uint32_t id = 0;
  if (!ArgNode(cx, args, 0, &id)) return false;
  const char* out = nullptr;
  size_t len = 0;
  mrt_dom_inner_text(tJs->host, id, &out, &len);
  return NewString(cx, out, len, args.rval());
}
static void CallWindowHook(JSContext* cx, JS::HandleObject global, const char* name, const JS::HandleValueArray& args);
// __mrtRefresh(op): the refresh tick for the frames (bridge.js). 0: whether a resize is pending; 1: the
// DOM's generation; 2: the documents that have frames flushed (the tick's layout phase); 3: the resize
// steps, each resized document's window gets its __mrt.fireResize() (PresShell::RunResizeSteps), in
// the order the sizes changed; returns how many.
NATIVE(Global_mrtRefresh) {
  JS::CallArgs args = JS::CallArgsFromVp(argc, vp);
  int32_t op = args.get(0).isNumber() ? int32_t(args.get(0).toNumber()) : -1;
  if (op != 3) {
    double r = mrt_dom_refresh(tJs->host, op, nullptr, 0);
    if (op == 0) args.rval().setBoolean(r != 0);
    else if (op == 1) args.rval().setNumber(r);
    else args.rval().setUndefined();
    return true;
  }
  std::vector<uint32_t> docs(64);
  size_t n = size_t(mrt_dom_refresh(tJs->host, 3, docs.data(), docs.size()));
  docs.resize(n);
  for (uint32_t doc : docs) {
    JS::RootedObject global(cx, WindowGlobalForDoc(doc));
    MrtJs::FrameWindow* w = global ? FrameWindowOf(global) : nullptr;
    if (global && (!w || (!w->closed && w->doc == doc))) CallWindowHook(cx, global, "fireResize", JS::HandleValueArray::empty());
  }
  args.rval().setInt32(int32_t(n));
  return true;
}
static std::vector<MrtJs::FrameWindow*> ChildWindows(uint32_t aDoc);
// The documents of window `aDoc` and of the open frame windows under it, each after its parent.
static void WindowTree(uint32_t aDoc, std::vector<uint32_t>& aOut) {
  aOut.push_back(aDoc);
  for (MrtJs::FrameWindow* w : ChildWindows(aDoc)) WindowTree(w->doc, aOut);
}
// __mrtEachWindow(name): `__mrt.<name>()` of every open window of the runtime, in the order of
// Gecko's pre-order walk of the browsing contexts (BrowsingContext::PreOrderWalk, which a tab's
// activeness change takes): the page's window, each frame's after its parent's, then the other root
// windows (frames of this site whose parents run elsewhere) the same way.
NATIVE(Global_mrtEachWindow) {
  JS::CallArgs args = JS::CallArgsFromVp(argc, vp);
  JS::RootedString str(cx, JS::ToString(cx, args.get(0)));
  if (!str) return false;
  JS::UniqueChars name = JS_EncodeStringToUTF8(cx, str);
  if (!name) return false;
  // the documents first: a hook may open or close a window
  std::vector<uint32_t> docs;
  WindowTree(1, docs);
  for (auto& w : tJs->windows)
    if (w.frame == 0 && !w.closed) WindowTree(w.doc, docs);
  for (uint32_t doc : docs) {
    JS::RootedObject global(cx, WindowGlobalForDoc(doc));
    if (global) CallWindowHook(cx, global, name.get(), JS::HandleValueArray::empty());
  }
  args.rval().setUndefined();
  return true;
}
// HTMLElement's offset members (Element::GetOffsetRect): each flushes layout and asks the layout
// thread; offsetParent is null without a frame or offset parent, the others 0 without a frame.
static bool OffsetRect(JSContext* cx, unsigned argc, JS::Value* vp, int which) {
  THIS_NODE;
  int32_t rect[4] = {0, 0, 0, 0};
  uint32_t parent = mrt_dom_offset_rect(tJs->host, self, rect);
  if (which < 0) {
    if (!parent) {
      args.rval().setNull();
      return true;
    }
    return Wrap(cx, parent, args.rval());
  }
  args.rval().setInt32(rect[which]);
  return true;
}
NATIVE(Element_offsetParent) { return OffsetRect(cx, argc, vp, -1); }
NATIVE(Element_offsetLeft) { return OffsetRect(cx, argc, vp, 0); }
NATIVE(Element_offsetTop) { return OffsetRect(cx, argc, vp, 1); }
NATIVE(Element_offsetWidth) { return OffsetRect(cx, argc, vp, 2); }
NATIVE(Element_offsetHeight) { return OffsetRect(cx, argc, vp, 3); }
NATIVE(Element_getBoundingClientRect) { return Box(cx, argc, vp); }

// ---- CharacterData ----

NATIVE(CharacterData_length) {
  THIS_NODE;
  const char* s;
  size_t len;
  mrt_dom_text_content(tJs->host, self, &s, &len);
  JS::RootedValue str(cx);
  if (!NewString(cx, s, len, &str)) return false;
  args.rval().setInt32(int32_t(JS::GetStringLength(str.toString())));
  return true;
}

// ---- Document ----

static bool DocumentChild(JSContext* cx, unsigned argc, JS::Value* vp, const char* tag) {
  THIS_NODE;
  uint32_t html = mrt_dom_link(tJs->host, self, 1);
  while (html && mrt_dom_kind(tJs->host, html) != KIND_ELEMENT) html = mrt_dom_link(tJs->host, html, 3);
  if (!tag) return Wrap(cx, html, args.rval());
  uint32_t c = html ? mrt_dom_link(tJs->host, html, 1) : 0;
  for (; c; c = mrt_dom_link(tJs->host, c, 3)) {
    const char* s;
    size_t len;
    if (mrt_dom_kind(tJs->host, c) == KIND_ELEMENT && mrt_dom_name(tJs->host, c, &s, &len) == NS_HTML &&
        len == strlen(tag) && !memcmp(s, tag, len)) {
      break;
    }
  }
  return Wrap(cx, c, args.rval());
}
NATIVE(Document_documentElement) { return DocumentChild(cx, argc, vp, nullptr); }
NATIVE(Document_body) { return DocumentChild(cx, argc, vp, "body"); }
NATIVE(Document_head) { return DocumentChild(cx, argc, vp, "head"); }
// Document::GetDoctype: the first child that is a DocumentType, or null.
NATIVE(Document_doctype) {
  THIS_NODE;
  uint32_t c = mrt_dom_link(tJs->host, self, 1);
  while (c && mrt_dom_kind(tJs->host, c) != KIND_DOCTYPE) c = mrt_dom_link(tJs->host, c, 3);
  return Wrap(cx, c, args.rval());
}
// Document::GetCompatMode: "BackCompat" in quirks mode, "CSS1Compat" otherwise (limited quirks too).
NATIVE(Document_compatMode) {
  JS::CallArgs args = JS::CallArgsFromVp(argc, vp);
  uint32_t self = NodeOf(args.thisv());
  const char* mode = mrt_dom_document_mode(tJs->host, self ? self : 1) == 2 ? "BackCompat" : "CSS1Compat";
  return NewString(cx, mode, strlen(mode), args.rval());
}
// DocumentType.name: the doctype's name as the parser lowercased it. publicId and systemId are
// empty: the parser shim does not keep them (HANDOFF, Deferred: parser gaps).
NATIVE(DocumentType_name) {
  THIS_NODE;
  const char* s;
  size_t len;
  mrt_dom_name(tJs->host, self, &s, &len);
  return NewString(cx, s, len, args.rval());
}
NATIVE(DocumentType_emptyId) {
  JS::CallArgs args = JS::CallArgsFromVp(argc, vp);
  args.rval().setString(JS_GetEmptyString(cx));
  return true;
}

NATIVE(Document_getElementById) {
  THIS_NODE;
  std::string id;
  if (!ArgUTF8(cx, args.get(0), id)) return false;
  return Wrap(cx, mrt_dom_get_element_by_id(tJs->host, self, id.data(), id.size()), args.rval());
}
// NS_NewHTMLElement from script: NewXULOrHTMLElement step 5.1, a defined name is constructed at
// once; if that throws, the exception is reported and the element is a plain one whose custom
// element state is failed
// A node just made by `doc`'s methods belongs to `doc` (nsINode::OwnerDoc); set before its reflector
// is made, whose global is its document's window.
static uint32_t OwnedBy(uint32_t doc, uint32_t id) {
  if (doc != 1 && id) mrt_dom_set_node_document(tJs->host, id, doc);
  return id;
}

static bool CreateHTMLElement(JSContext* cx, const std::string& tag, JS::MutableHandleValue rval, uint32_t doc = 1) {
  if (doc != 1) return Wrap(cx, OwnedBy(doc, mrt_dom_create_element(tJs->host, tag.data(), tag.size())), rval);
  if (CustomElementDefinition* definition = LookupCustomElementDefinition(tag)) {
    if (DoCustomElementCreate(cx, definition, rval)) return true;
    ReportCustomElementException(cx, "constructor");
    uint32_t id = mrt_dom_create_element(tJs->host, tag.data(), tag.size());
    mrt_dom_set_ce_state(tJs->host, id, CE_FAILED);
    return Wrap(cx, id, rval);
  }
  return Wrap(cx, mrt_dom_create_element(tJs->host, tag.data(), tag.size()), rval);
}
NATIVE(Document_createElement) {
  THIS_NODE;
  AutoCEReaction ceReaction(cx);
  std::string tag;
  if (!ArgUTF8(cx, args.get(0), tag)) return false;
  LowerASCII(tag);
  return CreateHTMLElement(cx, tag, args.rval(), self);
}

// nsContentUtils::IsValidElementLocalName and IsValidNamespacePrefix, over UTF-8 (bytes 0x80 and
// up stand for the non-ASCII characters both accept)
static bool IsHTMLWhitespaceByte(char c) { return c == ' ' || c == '\t' || c == '\n' || c == '\f' || c == '\r'; }
static bool IsAsciiAlphaByte(char c) { return (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z'); }
static bool IsValidElementLocalName(const std::string& aName) {
  if (aName.empty()) return false;
  unsigned char first = aName[0];
  if (IsAsciiAlphaByte(first)) {
    for (size_t i = 1; i < aName.size(); i++) {
      char c = aName[i];
      if (c == 0 || IsHTMLWhitespaceByte(c) || c == '/' || c == '>') return false;
    }
    return true;
  }
  if (first == ':' || first == '_' || first >= 0x80) {
    for (size_t i = 1; i < aName.size(); i++) {
      unsigned char c = aName[i];
      if (!(IsAsciiAlphaByte(c) || (c >= '0' && c <= '9') || c == '-' || c == '.' || c == ':' || c == '_' || c >= 0x80)) return false;
    }
    return true;
  }
  return false;
}
static bool IsValidNamespacePrefix(const std::string& aPrefix) {
  for (char c : aPrefix) {
    if (c == 0 || IsHTMLWhitespaceByte(c) || c == '/' || c == '>') return false;
  }
  return true;
}

// Document::CreateElementNS: the qualified name checked as nsContentUtils::GetNodeInfoFromQName
// (ParseQualifiedNameRelaxed, then IsValidNodeName), the case kept; an HTML element goes through
// the custom element steps as createElement does. The arena keeps the parser's namespaces only and
// no prefix: an element in another namespace has none, and a prefixed name keeps its local name
// (GAPS B18).
NATIVE(Document_createElementNS) {
  THIS_NODE;
  AutoCEReaction ceReaction(cx);
  std::string nsURI, qname;
  if (!args.get(0).isNullOrUndefined() && !ArgUTF8(cx, args.get(0), nsURI)) return false;
  if (!ArgUTF8(cx, args.get(1), qname)) return false;
  std::string prefix, local = qname;
  bool hasPrefix = false;
  size_t colon = qname.find(':');
  if (colon != std::string::npos) {
    hasPrefix = true;
    prefix = qname.substr(0, colon);
    local = qname.substr(colon + 1);
    if (prefix.empty() || !IsValidNamespacePrefix(prefix) || local.empty() || !IsValidElementLocalName(local)) {
      return ThrowNamedDOMException(cx, "InvalidCharacterError", "String contains an invalid character");
    }
  } else if (!IsValidElementLocalName(qname)) {
    return ThrowNamedDOMException(cx, "InvalidCharacterError", "String contains an invalid character");
  }
  // the parser's namespace numbers (nsNameSpaceManager.h in the shim)
  int32_t ns = nsURI.empty() ? 0
             : nsURI == "http://www.w3.org/2000/xmlns/" ? 1
             : nsURI == "http://www.w3.org/XML/1998/namespace" ? 2
             : nsURI == "http://www.w3.org/1999/xhtml" ? NS_HTML
             : nsURI == "http://www.w3.org/1999/xlink" ? 4
             : nsURI == "http://www.w3.org/1998/Math/MathML" ? 5
             : nsURI == "http://www.w3.org/2000/svg" ? 6 : -1;
  // IsValidNodeName (an unknown namespace is registered in Gecko; here it has none)
  bool valid = !hasPrefix ? ((local == "xmlns") == (ns == 1))
             : nsURI.empty() ? false
             : ns == 1 ? (prefix == "xmlns" && local != "xmlns")
             : prefix != "xmlns" && (ns == 2 || prefix != "xml");
  if (!valid) {
    return ThrowNamedDOMException(cx, "NamespaceError", "An attempt was made to create or change an object in a way which is incorrect with regard to namespaces");
  }
  if (ns == NS_HTML) return CreateHTMLElement(cx, local, args.rval());
  return Wrap(cx, OwnedBy(self, mrt_dom_create_element_ns(tJs->host, ns < 0 ? 0 : ns, local.data(), local.size())), args.rval());
}
NATIVE(Document_createTextNode) {
  THIS_NODE;
  std::string text;
  if (!ArgUTF8(cx, args.get(0), text)) return false;
  return Wrap(cx, OwnedBy(self, mrt_dom_create_text(tJs->host, text.data(), text.size())), args.rval());
}

NATIVE(Document_createComment) {
  THIS_NODE;
  std::string text;
  if (!ArgUTF8(cx, args.get(0), text)) return false;
  return Wrap(cx, OwnedBy(self, mrt_dom_create_node(tJs->host, 3, text.data(), text.size())), args.rval());
}
NATIVE(Document_createDocumentFragment) {
  THIS_NODE;
  return Wrap(cx, OwnedBy(self, mrt_dom_create_node(tJs->host, 5, nullptr, 0)), args.rval());
}
NATIVE(Node_cloneNode) {
  THIS_NODE;
  AutoCEReaction ceReaction(cx);
  if (mrt_dom_kind(tJs->host, self) == KIND_SHADOW_ROOT) {
    JS_ReportErrorUTF8(cx, "NotSupportedError: ShadowRoot nodes are not clonable.");
    return false;
  }
  uint32_t clone = mrt_dom_clone(tJs->host, self, JS::ToBoolean(args.get(0)));
  EnqueueUpgradesForNewSubtree(clone);
  return Wrap(cx, clone, args.rval());
}
// document.importNode(node, deep): a copy for this document. All documents share one arena here,
// so it is cloneNode's copy, with upgrade reactions for its defined custom elements (Gecko clones it
// with synchronous custom elements off). A document cannot be imported.
NATIVE(Document_importNode) {
  JS::CallArgs args = JS::CallArgsFromVp(argc, vp);
  AutoCEReaction ceReaction(cx);
  uint32_t node;
  if (!ArgNode(cx, args, 0, &node)) return false;
  if (mrt_dom_kind(tJs->host, node) == KIND_DOCUMENT) {
    JS_ReportErrorUTF8(cx, "NotSupportedError: Operation is not supported");
    return false;
  }
  uint32_t clone = mrt_dom_clone(tJs->host, node, JS::ToBoolean(args.get(1)));
  // the copy belongs to the importing document (Document::ImportNode)
  if (uint32_t doc = NodeOf(args.thisv())) mrt_dom_adopt(tJs->host, clone, doc);
  EnqueueUpgradesForNewSubtree(clone);
  return Wrap(cx, clone, args.rval());
}

// document.adoptNode(node): the node taken out of its parent (Gecko's AdoptNode; the owner document
// does not change, one arena). A document cannot be adopted.
NATIVE(Document_adoptNode) {
  JS::CallArgs args = JS::CallArgsFromVp(argc, vp);
  AutoCEReaction ceReaction(cx);
  uint32_t node;
  if (!ArgNode(cx, args, 0, &node)) return false;
  if (mrt_dom_kind(tJs->host, node) == KIND_DOCUMENT) {
    JS_ReportErrorUTF8(cx, "NotSupportedError: Operation is not supported");
    return false;
  }
  if (uint32_t parent = ParentNode(node)) RemoveChildInternal(cx, parent, node);
  if (uint32_t doc = NodeOf(args.thisv())) mrt_dom_adopt(tJs->host, node, doc);
  args.rval().set(args.get(0));
  return true;
}

NATIVE(Element_getAttributeNames) {
  THIS_NODE;
  const char* s;
  size_t len;
  mrt_dom_attr_names(tJs->host, self, &s, &len);
  JS::RootedObject arr(cx, JS::NewArrayObject(cx, 0));
  if (!arr) return false;
  uint32_t n = 0;
  size_t start = 0;
  for (size_t i = 0; i <= len; i++) {
    if (i == len || s[i] == '\n') {
      if (i > start) {
        JS::RootedValue v(cx);
        if (!NewString(cx, s + start, i - start, &v) || !JS_SetElement(cx, arr, n++, v)) return false;
      }
      start = i + 1;
    }
  }
  args.rval().setObject(*arr);
  return true;
}
// __mrtParseFragment(html): a new DocumentFragment holding the parsed markup (glue/bridge.js uses it
// for innerHTML, outerHTML and insertAdjacentHTML).
// __mrtParseFragment(html, context): a new fragment of `html` parsed in the context of element
// `context` (nsContentUtils::ParseFragmentHTML), or of a <body> when it is not given.
NATIVE(Global_mrtParseFragment) {
  JS::CallArgs args = JS::CallArgsFromVp(argc, vp);
  std::string html;
  if (!ArgUTF8(cx, args.get(0), html)) return false;
  uint32_t context = 0;
  if (args.get(1).isObject() && !ArgNode(cx, args, 1, &context)) return false;
  uint32_t fragment = mrt_dom_parse_fragment(tJs->host, html.data(), html.size(), context);
  EnqueueUpgradesForNewSubtree(fragment);
  return Wrap(cx, fragment, args.rval());
}

// ---- HTMLTemplateElement ----

NATIVE(HTMLTemplateElement_content) {
  THIS_NODE;
  return Wrap(cx, mrt_dom_template_contents(tJs->host, self), args.rval());
}
static const JSPropertySpec kHTMLTemplateElementProps[] = {
    JS_PSG("content", HTMLTemplateElement_content, JSPROP_ENUMERATE), JS_PS_END};

// ---- shadow DOM: the tree side (dom/base/Element.cpp, ShadowRoot.cpp, HTMLSlotElement.cpp) ----

static CustomElementDefinition* LookupCustomElementDefinition(const std::string& aLocalName);
static CustomElementDefinition* GetCustomElementDefinition(uint32_t aElement);

// Element::AttachShadow: `init.mode` is required ("open" or "closed", a TypeError otherwise, as the
// WebIDL enum conversion throws); NotSupportedError for an element that cannot host one
// (CanAttachShadowDOM, including a custom element definition's disabledFeatures: ["shadow"]) or
// that already has one that is not declarative of the same mode.
NATIVE(Element_attachShadow) {
  THIS_NODE;
  if (!args.get(0).isObject()) {
    JS_ReportErrorUTF8(cx, "TypeError: Element.attachShadow: Argument 1 is not an object.");
    return false;
  }
  JS::RootedObject init(cx, &args[0].toObject());
  JS::RootedValue v(cx);
  std::string mode, slotAssignment;
  if (!JS_GetProperty(cx, init, "mode", &v)) return false;
  if (v.isUndefined()) {
    JS_ReportErrorUTF8(cx, "TypeError: Element.attachShadow: Missing required 'mode' member of ShadowRootInit.");
    return false;
  }
  if (!ArgUTF8(cx, v, mode)) return false;
  uint32_t flags;
  if (mode == "open") {
    flags = 0;
  } else if (mode == "closed") {
    flags = SR_CLOSED;
  } else {
    JS_ReportErrorUTF8(cx, "TypeError: Element.attachShadow: '%s' (value of 'mode' member of ShadowRootInit) is not a valid value for enumeration ShadowRootMode.", mode.c_str());
    return false;
  }
  static const std::pair<const char*, uint32_t> kBools[] = {
      {"clonable", SR_CLONABLE}, {"serializable", SR_SERIALIZABLE}, {"delegatesFocus", SR_DELEGATES_FOCUS}};
  for (auto& [name, bit] : kBools) {
    if (!JS_GetProperty(cx, init, name, &v)) return false;
    if (JS::ToBoolean(v)) flags |= bit;
  }
  if (!JS_GetProperty(cx, init, "slotAssignment", &v)) return false;
  if (!v.isUndefined()) {
    if (!ArgUTF8(cx, v, slotAssignment)) return false;
    if (slotAssignment == "manual") {
      flags |= SR_MANUAL_SLOTS;
    } else if (slotAssignment != "named") {
      JS_ReportErrorUTF8(cx, "TypeError: Element.attachShadow: '%s' (value of 'slotAssignment' member of ShadowRootInit) is not a valid value for enumeration SlotAssignmentMode.", slotAssignment.c_str());
      return false;
    }
  }
  // CanAttachShadowDOM step 3: the definition, or the one the name would get if not yet upgraded
  if (mrt_dom_kind(tJs->host, self) == KIND_ELEMENT && mrt_dom_ce_state(tJs->host, self) != CE_UNCUSTOMIZED) {
    CustomElementDefinition* definition = GetCustomElementDefinition(self);
    if (!definition) definition = LookupCustomElementDefinition(LocalName(self));
    if (definition && definition->mDisableShadow) {
      JS_ReportErrorUTF8(cx, "NotSupportedError: Unable to attach ShadowDOM");
      return false;
    }
  }
  bool hadRoot = mrt_dom_link(tJs->host, self, LINK_SHADOW_ROOT) != 0;
  int64_t root = mrt_dom_attach_shadow(tJs->host, self, flags);
  if (root < 0) {
    JS_ReportErrorUTF8(cx, hadRoot ? "NotSupportedError: Unable to re-attach to existing ShadowDOM"
                                   : "NotSupportedError: Unable to attach ShadowDOM");
    return false;
  }
  return Wrap(cx, uint32_t(root), args.rval());
}

// shadowRoot: an open shadow root, null for a closed one (Element::GetShadowRootByMode).
NATIVE(Element_shadowRoot) {
  THIS_NODE;
  uint32_t root = mrt_dom_link(tJs->host, self, LINK_SHADOW_ROOT);
  if (root && (mrt_dom_shadow_flags(tJs->host, root) & SR_CLOSED)) root = 0;
  return Wrap(cx, root, args.rval());
}

// assignedSlot (Slottable, on Element and Text): the slot, null when it is in a closed shadow root
// (nsIContent::GetAssignedSlotByMode).
NATIVE(Slottable_assignedSlot) {
  THIS_NODE;
  uint32_t slot = mrt_dom_link(tJs->host, self, LINK_ASSIGNED_SLOT);
  if (slot && (mrt_dom_shadow_flags(tJs->host, ContainingShadow(slot)) & SR_CLOSED)) slot = 0;
  return Wrap(cx, slot, args.rval());
}

static bool ShadowFlag(JSContext* cx, unsigned argc, JS::Value* vp, uint32_t aFlag) {
  THIS_NODE;
  int32_t flags = mrt_dom_shadow_flags(tJs->host, self);
  if (flags < 0) {
    JS_ReportErrorUTF8(cx, "TypeError: 'this' is not a ShadowRoot");
    return false;
  }
  args.rval().setBoolean(flags & aFlag);
  return true;
}
NATIVE(ShadowRoot_host) {
  THIS_NODE;
  return Wrap(cx, mrt_dom_link(tJs->host, self, LINK_HOST), args.rval());
}
NATIVE(ShadowRoot_mode) {
  THIS_NODE;
  bool closed = mrt_dom_shadow_flags(tJs->host, self) & SR_CLOSED;
  return NewString(cx, closed ? "closed" : "open", closed ? 6 : 4, args.rval());
}
NATIVE(ShadowRoot_slotAssignment) {
  THIS_NODE;
  bool manual = mrt_dom_shadow_flags(tJs->host, self) & SR_MANUAL_SLOTS;
  return NewString(cx, manual ? "manual" : "named", manual ? 6 : 5, args.rval());
}
NATIVE(ShadowRoot_clonable) { return ShadowFlag(cx, argc, vp, SR_CLONABLE); }
NATIVE(ShadowRoot_serializable) { return ShadowFlag(cx, argc, vp, SR_SERIALIZABLE); }
NATIVE(ShadowRoot_delegatesFocus) { return ShadowFlag(cx, argc, vp, SR_DELEGATES_FOCUS); }
// No focus in the mini runtime: nothing is ever active.
NATIVE(ShadowRoot_activeElement) {
  JS::CallArgs args = JS::CallArgsFromVp(argc, vp);
  args.rval().setNull();
  return true;
}
static const JSPropertySpec kShadowRootProps[] = {
    JS_PSG("host", ShadowRoot_host, JSPROP_ENUMERATE),
    JS_PSG("mode", ShadowRoot_mode, JSPROP_ENUMERATE),
    JS_PSG("delegatesFocus", ShadowRoot_delegatesFocus, JSPROP_ENUMERATE),
    JS_PSG("slotAssignment", ShadowRoot_slotAssignment, JSPROP_ENUMERATE),
    JS_PSG("clonable", ShadowRoot_clonable, JSPROP_ENUMERATE),
    JS_PSG("serializable", ShadowRoot_serializable, JSPROP_ENUMERATE),
    JS_PSG("activeElement", ShadowRoot_activeElement, JSPROP_ENUMERATE),
    JS_PS_END};
static bool Document_getElementById(JSContext* cx, unsigned argc, JS::Value* vp);
static const JSFunctionSpec kShadowRootFns[] = {
    JS_FN("getElementById", Document_getElementById, 1, JSPROP_ENUMERATE), JS_FS_END};

// HTMLSlotElement.assignedNodes and assignedElements, with `flatten` (HTMLSlotElement::AssignedNodes:
// flattened, a slot in the list is replaced by its own flattened assigned nodes, and a slot with
// none gives its children, flattened the same way).
static void FlattenAssignedNodes(uint32_t aSlot, std::vector<uint32_t>& aOut, bool aFlatten, bool aTop) {
  std::vector<uint32_t> nodes;
  int32_t n = mrt_dom_assigned_nodes(tJs->host, aSlot);
  for (int32_t i = 0; i < n; i++) nodes.push_back(mrt_dom_result(tJs->host, i));
  if (aFlatten && nodes.empty() && (!aTop || ContainingShadow(aSlot))) {
    // a slot with no assigned nodes stands for its children (fallback content)
    for (uint32_t c = mrt_dom_link(tJs->host, aSlot, LINK_FIRST); c; c = mrt_dom_link(tJs->host, c, LINK_NEXT)) {
      int kind = mrt_dom_kind(tJs->host, c);
      if (kind == KIND_ELEMENT || kind == KIND_TEXT) nodes.push_back(c);
    }
  }
  for (uint32_t node : nodes) {
    if (aFlatten && IsSlot(node) && ContainingShadow(node)) {
      FlattenAssignedNodes(node, aOut, true, false);
    } else {
      aOut.push_back(node);
    }
  }
}
static bool AssignedNodes(JSContext* cx, unsigned argc, JS::Value* vp, bool aElementsOnly) {
  THIS_NODE;
  bool flatten = false;
  if (args.get(0).isObject()) {
    JS::RootedObject opts(cx, &args[0].toObject());
    JS::RootedValue v(cx);
    if (!JS_GetProperty(cx, opts, "flatten", &v)) return false;
    flatten = JS::ToBoolean(v);
  }
  std::vector<uint32_t> nodes;
  if (IsSlot(self)) FlattenAssignedNodes(self, nodes, flatten, true);
  JS::RootedObject arr(cx, JS::NewArrayObject(cx, 0));
  if (!arr) return false;
  JS::RootedValue item(cx);
  uint32_t i = 0;
  for (uint32_t node : nodes) {
    if (aElementsOnly && mrt_dom_kind(tJs->host, node) != KIND_ELEMENT) continue;
    if (!Wrap(cx, node, &item) || !JS_SetElement(cx, arr, i++, item)) return false;
  }
  args.rval().setObject(*arr);
  return true;
}
NATIVE(HTMLSlotElement_assignedNodes) { return AssignedNodes(cx, argc, vp, false); }
NATIVE(HTMLSlotElement_assignedElements) { return AssignedNodes(cx, argc, vp, true); }
// Manual slot assignment is not built (GAPS B25): assign() keeps nothing.
NATIVE(HTMLSlotElement_assign) {
  JS::CallArgs args = JS::CallArgsFromVp(argc, vp);
  args.rval().setUndefined();
  return true;
}
static const JSFunctionSpec kHTMLSlotElementFns[] = {
    JS_FN("assignedNodes", HTMLSlotElement_assignedNodes, 0, JSPROP_ENUMERATE),
    JS_FN("assignedElements", HTMLSlotElement_assignedElements, 0, JSPROP_ENUMERATE),
    JS_FN("assign", HTMLSlotElement_assign, 0, JSPROP_ENUMERATE),
    JS_FS_END};

// ---- custom elements: the constructor and the registry ----

// The reflector of a new or upgraded custom element with the prototype its constructor asked for
// (GetOrCreateDOMReflector with a given prototype). Preserved, as Gecko preserves a reflector whose
// prototype is not its interface's, so the prototype and the constructor's fields survive GC.
static bool WrapWithProto(JSContext* cx, uint32_t id, JS::HandleObject proto, JS::MutableHandleValue rval) {
  MrtJs* js = tJs;
  if (!(id < js->cache.size() && js->cache[id])) {
    JS::RootedObject obj(cx, JS_NewObjectWithGivenProto(cx, &kReflectorClass, proto));
    if (!obj) return false;
    JS_SetReservedSlot(obj, 0, JS::Int32Value(int32_t(id)));
    if (id >= js->cache.size()) {
      size_t n = std::max<size_t>(id + 1, js->cache.size() * 2);
      js->cache.resize(n);
      js->preserved.resize(n);
    }
    js->cache[id] = obj;
  }
  js->preserved[id] = 1;
  rval.setObject(*js->cache[id].get());
  return true;
}

// HTMLConstructor (dom/bindings/BindingUtils.cpp): `HTMLElement` called as `super()` from a custom
// element's constructor. Autonomous custom elements only (no `extends`).
static bool HTMLConstructor(JSContext* cx, unsigned argc, JS::Value* vp) {
  JS::CallArgs args = JS::CallArgsFromVp(argc, vp);
  if (!args.isConstructing()) {
    JS_ReportErrorUTF8(cx, "TypeError: HTMLElement constructor: 'new' is required");
    return false;
  }
  CustomElements& ce = tJs->ce;
  JS::RootedObject newTarget(cx, &args.newTarget().toObject());
  // 1. NewTarget is HTMLElement itself.
  if (newTarget == &args.callee()) {
    JS_ReportErrorUTF8(cx, "TypeError: Illegal constructor.");
    return false;
  }
  // 5. The definition whose constructor is NewTarget.
  CustomElementDefinition* definition = LookupCustomElementDefinition(newTarget);
  if (!definition) {
    JS_ReportErrorUTF8(cx, "TypeError: Illegal constructor.");
    return false;
  }
  // 10-11. GetDesiredProto: NewTarget.prototype, or HTMLElement.prototype if it is not an object.
  JS::RootedValue protoVal(cx);
  if (!JS_GetProperty(cx, newTarget, "prototype", &protoVal)) return false;
  JS::RootedObject global(cx, JS::CurrentGlobalOrNull(cx));
  JS::RootedObject desiredProto(cx, protoVal.isObject() ? &protoVal.toObject()
                                                        : &JS::GetReservedSlot(global, SLOT_HTML_ELEMENT).toObject());
  uint32_t element;
  if (definition->mConstructionStack.empty()) {
    // 9. Direct construction (`new X()`, or createElement of a defined name): a new element,
    // custom, with this definition.
    element = mrt_dom_create_element(tJs->host, definition->mLocalName.data(), definition->mLocalName.size());
    mrt_dom_set_ce_state(tJs->host, element, CE_CUSTOM);
    ce.mElementDefinition[element] = definition;
  } else {
    // 12-15. An upgrade: the element being upgraded takes the constructor's prototype.
    element = definition->mConstructionStack.back();
    if (element == ALREADY_CONSTRUCTED_MARKER) {
      JS_ReportErrorUTF8(cx, "TypeError: Cannot instantiate a custom element inside its own constructor during upgrades");
      return false;
    }
    if (element < tJs->cache.size() && tJs->cache[element]) {
      JS::RootedObject reflector(cx, tJs->cache[element].get());
      if (!JS_SetPrototype(cx, reflector, desiredProto)) return false;
    }
    definition->mConstructionStack.back() = ALREADY_CONSTRUCTED_MARKER;
  }
  // 16.
  return WrapWithProto(cx, element, desiredProto, args.rval());
}

// DoCustomElementCreate (nsContentUtils.cpp): createElement of a defined name runs its constructor
// at once and checks what it made.
static bool DoCustomElementCreate(JSContext* cx, CustomElementDefinition* aDefinition, JS::MutableHandleValue rval) {
  JS::RootedValue constructor(cx, JS::ObjectValue(*aDefinition->mConstructor.get()));
  JS::RootedObject result(cx);
  if (!JS::Construct(cx, constructor, JS::HandleValueArray::empty(), &result)) return false;
  rval.setObject(*result);
  uint32_t element = NodeOf(rval);
  const char* name;
  size_t len;
  if (!element || mrt_dom_kind(tJs->host, element) != KIND_ELEMENT || mrt_dom_name(tJs->host, element, &name, &len) != NS_HTML) {
    JS_ReportErrorUTF8(cx, "TypeError: \"this\" doesn't implement interface HTMLElement.");
    return false;
  }
  const char* attrs;
  size_t attrs_len;
  mrt_dom_attr_names(tJs->host, element, &attrs, &attrs_len);
  if (mrt_dom_link(tJs->host, element, 0) || mrt_dom_link(tJs->host, element, 1) || attrs_len ||
      LocalName(element) != aDefinition->mLocalName) {
    JS_ReportErrorUTF8(cx, "NotSupportedError: Operation is not supported");
    return false;
  }
  return true;
}

// The CustomElementRegistry methods take `this` as given: the page has one registry.

// The string sequence in property `aProp` of the constructor (JSObjectToAtomArray), for
// observedAttributes and disabledFeatures.
static bool JSObjectToStringArray(JSContext* cx, JS::HandleObject aConstructor, const char* aProp,
                                  std::vector<std::string>& aArray) {
  JS::RootedValue iterable(cx);
  if (!JS_GetProperty(cx, aConstructor, aProp, &iterable)) return false;
  if (iterable.isUndefined()) return true;
  JS::ForOfIterator iter(cx);
  if (!iter.init(iterable, JS::ForOfIterator::AllowNonIterable)) return false;
  if (!iter.valueIsIterable()) {
    JS_ReportErrorUTF8(cx, "TypeError: CustomElementRegistry.define: %s can't be converted to a sequence.", aProp);
    return false;
  }
  JS::RootedValue v(cx);
  while (true) {
    bool done;
    if (!iter.next(&v, &done)) return false;
    if (done) break;
    std::string s;
    if (!ArgUTF8(cx, v, s)) return false;
    aArray.push_back(std::move(s));
  }
  return true;
}

// One lifecycle callback from the prototype: null if undefined, else it must be callable.
static bool GetLifecycleCallback(JSContext* cx, JS::HandleObject aPrototype, const char* aName,
                                 JS::Heap<JSObject*>* aOut) {
  JS::RootedValue v(cx);
  if (!JS_GetProperty(cx, aPrototype, aName, &v)) return false;
  if (v.isUndefined()) return true;
  if (!v.isObject() || !JS::IsCallable(&v.toObject())) {
    JS_ReportErrorUTF8(cx, "TypeError: CustomElementRegistry.define: '%s' is not a function.", aName);
    return false;
  }
  if (aOut) *aOut = &v.toObject();
  return true;
}

// CustomElementRegistry::Define (https://html.spec.whatwg.org/#dom-customelementregistry-define).
NATIVE(CustomElementRegistry_define) {
  JS::CallArgs args = JS::CallArgsFromVp(argc, vp);
  AutoCEReaction ceReaction(cx);
  CustomElements& ce = tJs->ce;
  std::string name;
  if (!ArgUTF8(cx, args.get(0), name)) return false;
  // 1.
  if (!args.get(1).isObject() || !JS::IsConstructor(&args.get(1).toObject())) {
    JS_ReportErrorUTF8(cx, "TypeError: CustomElementRegistry.define: Argument 2 is not a constructor.");
    return false;
  }
  JS::RootedObject constructor(cx, &args.get(1).toObject());
  // 2-4.
  if (!mrt_dom_is_custom_element_name(name.data(), name.size())) {
    JS_ReportErrorUTF8(cx, "SyntaxError: '%s' is not a valid custom element name", name.c_str());
    return false;
  }
  if (ce.mCustomDefinitions.count(name)) {
    JS_ReportErrorUTF8(cx, "NotSupportedError: '%s' has already been defined as a custom element", name.c_str());
    return false;
  }
  if (CustomElementDefinition* other = LookupCustomElementDefinition(constructor)) {
    JS_ReportErrorUTF8(cx, "NotSupportedError: '%s' and '%s' have the same constructor", name.c_str(), other->mType.c_str());
    return false;
  }
  // 5-7. `extends` (customized built-in elements) is not supported: one prototype for every HTML
  // element (GAPS B11).
  if (args.get(2).isObject()) {
    JS::RootedObject options(cx, &args.get(2).toObject());
    JS::RootedValue extends(cx);
    if (!JS_GetProperty(cx, options, "extends", &extends)) return false;
    if (!extends.isUndefined()) {
      JS_ReportErrorUTF8(cx, "NotSupportedError: customized built-in elements are not supported (mini runtime)");
      return false;
    }
  }
  // 8.
  if (ce.mIsCustomDefinitionRunning) {
    JS_ReportErrorUTF8(cx, "NotSupportedError: Cannot define a custom element while defining another custom element");
    return false;
  }
  auto definition = std::make_unique<CustomElementDefinition>();
  definition->mType = name;
  definition->mLocalName = name;
  {
    // 9-14, with the "running" flag set (AutoRestore).
    struct Restore {
      bool& flag;
      ~Restore() { flag = false; }
    } restore{ce.mIsCustomDefinitionRunning};
    ce.mIsCustomDefinitionRunning = true;
    JS::RootedValue prototype(cx);
    if (!JS_GetProperty(cx, constructor, "prototype", &prototype)) return false;
    if (!prototype.isObject()) {
      JS_ReportErrorUTF8(cx, "TypeError: CustomElementRegistry.define: constructor.prototype is not an object.");
      return false;
    }
    JS::RootedObject proto(cx, &prototype.toObject());
    // the LifecycleCallbacks dictionary's members, read in its (alphabetical) order
    if (!GetLifecycleCallback(cx, proto, "adoptedCallback", &definition->mAdoptedCallback) ||
        !GetLifecycleCallback(cx, proto, "attributeChangedCallback", &definition->mAttributeChangedCallback) ||
        !GetLifecycleCallback(cx, proto, "connectedCallback", &definition->mConnectedCallback) ||
        !GetLifecycleCallback(cx, proto, "connectedMoveCallback", nullptr) ||
        !GetLifecycleCallback(cx, proto, "disconnectedCallback", &definition->mDisconnectedCallback)) {
      return false;
    }
    if (definition->mAttributeChangedCallback &&
        !JSObjectToStringArray(cx, constructor, "observedAttributes", definition->mObservedAttributes)) {
      return false;
    }
    std::vector<std::string> disabledFeatures;
    if (!JSObjectToStringArray(cx, constructor, "disabledFeatures", disabledFeatures)) return false;
    for (const std::string& f : disabledFeatures) {
      if (f == "internals") definition->mDisableInternals = true;
      if (f == "shadow") definition->mDisableShadow = true;
    }
    JS::RootedValue formAssociated(cx);
    if (!JS_GetProperty(cx, constructor, "formAssociated", &formAssociated)) return false;
    definition->mFormAssociated = JS::ToBoolean(formAssociated);
    if (definition->mFormAssociated) {
      // read and checked as Gecko does; form-associated custom elements are not supported further
      for (const char* cb : {"formAssociatedCallback", "formDisabledCallback", "formResetCallback", "formStateRestoreCallback"}) {
        if (!GetLifecycleCallback(cx, proto, cb, nullptr)) return false;
      }
    }
  }
  // 15-16.
  definition->mConstructor = constructor;
  CustomElementDefinition* def = definition.get();
  ce.mDefinitions.push_back(std::move(definition));
  ce.mCustomDefinitions[name] = def;
  // 18. UpgradeCandidates: the connected undefined elements of that name, in tree order.
  for (uint32_t n = 1; n; n = NextInSubtree(n, 1)) {
    if (mrt_dom_ce_state(tJs->host, n) == CE_UNDEFINED && LocalName(n) == name) EnqueueUpgradeReaction(n, def);
  }
  // 19. whenDefined's promise.
  auto it = ce.mWhenDefinedPromiseMap.find(name);
  if (it != ce.mWhenDefinedPromiseMap.end()) {
    JS::RootedObject promise(cx, it->second.get());
    ce.mWhenDefinedPromiseMap.erase(it);
    JS::RootedValue ctor(cx, JS::ObjectValue(*constructor));
    if (!JS::ResolvePromise(cx, promise, ctor)) return false;
  }
  args.rval().setUndefined();
  return true;
}

NATIVE(CustomElementRegistry_get) {
  JS::CallArgs args = JS::CallArgsFromVp(argc, vp);
  std::string name;
  if (!ArgUTF8(cx, args.get(0), name)) return false;
  CustomElementDefinition* definition = LookupCustomElementDefinition(name);
  if (definition) {
    args.rval().setObject(*definition->mConstructor.get());
  } else {
    args.rval().setUndefined();
  }
  return true;
}

NATIVE(CustomElementRegistry_getName) {
  JS::CallArgs args = JS::CallArgsFromVp(argc, vp);
  CustomElementDefinition* definition = args.get(0).isObject() ? LookupCustomElementDefinition(&args.get(0).toObject()) : nullptr;
  if (definition) return NewString(cx, definition->mType.data(), definition->mType.size(), args.rval());
  args.rval().setNull();
  return true;
}

// CustomElementRegistry::WhenDefined; its errors become a rejected promise, as for any WebIDL
// method that returns one.
NATIVE(CustomElementRegistry_whenDefined) {
  JS::CallArgs args = JS::CallArgsFromVp(argc, vp);
  CustomElements& ce = tJs->ce;
  std::string name;
  if (!ArgUTF8(cx, args.get(0), name)) return false;
  JS::RootedObject promise(cx);
  if (!mrt_dom_is_custom_element_name(name.data(), name.size())) {
    JS_ReportErrorUTF8(cx, "SyntaxError: '%s' is not a valid custom element name", name.c_str());
    JS::RootedValue exn(cx);
    if (!JS_GetPendingException(cx, &exn)) return false;
    JS_ClearPendingException(cx);
    promise = JS::CallOriginalPromiseReject(cx, exn);
  } else if (CustomElementDefinition* definition = LookupCustomElementDefinition(name)) {
    JS::RootedValue ctor(cx, JS::ObjectValue(*definition->mConstructor.get()));
    promise = JS::CallOriginalPromiseResolve(cx, ctor);
  } else {
    auto it = ce.mWhenDefinedPromiseMap.find(name);
    if (it != ce.mWhenDefinedPromiseMap.end()) {
      promise = it->second.get();
    } else {
      promise = JS::NewPromiseObject(cx, nullptr);
      if (promise) ce.mWhenDefinedPromiseMap[name] = promise.get();
    }
  }
  if (!promise) return false;
  args.rval().setObject(*promise);
  return true;
}

// CustomElementRegistry::Upgrade(nsINode&): an upgrade reaction for each custom element of the
// subtree whose name is defined.
NATIVE(CustomElementRegistry_upgrade) {
  JS::CallArgs args = JS::CallArgsFromVp(argc, vp);
  AutoCEReaction ceReaction(cx);
  uint32_t root;
  if (!ArgNode(cx, args, 0, &root)) return false;
  for (uint32_t n = root; n; n = NextInSubtree(n, root)) {
    int32_t state = mrt_dom_ce_state(tJs->host, n);
    if (state <= CE_UNCUSTOMIZED) continue;
    if (CustomElementDefinition* definition = LookupCustomElementDefinition(LocalName(n))) {
      EnqueueUpgradeReaction(n, definition);
    }
  }
  args.rval().setUndefined();
  return true;
}

// __mrtCreateHTMLDocument(title): DOMImplementation.createHTMLDocument's new document (prelude.js);
// no title element when `title` is undefined.
NATIVE(Global_mrtCreateHTMLDocument) {
  JS::CallArgs args = JS::CallArgsFromVp(argc, vp);
  std::string title;
  bool has_title = !args.get(0).isUndefined();
  if (has_title && !ArgUTF8(cx, args.get(0), title)) return false;
  return Wrap(cx, mrt_dom_create_html_document(tJs->host, title.data(), title.size(), has_title), args.rval());
}

// __mrtSheet(op, ...): constructed style sheets for bridge.js's CSSStyleSheet. op 0 (text): a new
// sheet, its id; 1 (id, text): its new text; 2 (node, [ids]): a document's or shadow root's
// adoptedStyleSheets.
NATIVE(Global_mrtSheet) {
  JS::CallArgs args = JS::CallArgsFromVp(argc, vp);
  int op = args.get(0).isInt32() ? args.get(0).toInt32() : -1;
  args.rval().setUndefined();
  if (op == 0 || op == 1) {
    std::string css;
    if (!ArgUTF8(cx, args.get(op == 0 ? 1 : 2), css)) return false;
    if (op == 0) {
      args.rval().setNumber(double(mrt_dom_new_sheet(tJs->host, css.data(), css.size())));
    } else {
      mrt_dom_set_sheet(tJs->host, uint32_t(args.get(1).toNumber()), css.data(), css.size());
    }
    return true;
  }
  if (op == 2) {
    uint32_t node;
    if (!ArgNode(cx, args, 1, &node)) return false;
    std::vector<uint32_t> ids;
    if (args.get(2).isObject()) {
      JS::RootedObject list(cx, &args[2].toObject());
      uint32_t n = 0;
      if (!JS::GetArrayLength(cx, list, &n)) return false;
      for (uint32_t i = 0; i < n; i++) {
        JS::RootedValue v(cx);
        if (!JS_GetElement(cx, list, i, &v)) return false;
        if (v.isNumber()) ids.push_back(uint32_t(v.toNumber()));
      }
    }
    mrt_dom_set_adopted(tJs->host, node, ids.data(), ids.size());
    return true;
  }
  JS_ReportErrorASCII(cx, "__mrtSheet: bad op");
  return false;
}

// __mrtCssom(op, sheet, text, n): mrt_cssom_op for bridge.js's CSSStyleSheet; `sheet` is a <style>
// or <link> element, or a constructed sheet's id. The result string.
NATIVE(Global_mrtCssom) {
  JS::CallArgs args = JS::CallArgsFromVp(argc, vp);
  int32_t op = 0;
  if (!JS::ToInt32(cx, args.get(0), &op)) return false;
  uint32_t id = 0;
  int32_t owner = 0;
  if (args.get(1).isNumber()) {
    id = uint32_t(args.get(1).toNumber());
  } else {
    if (!ArgNode(cx, args, 1, &id)) return false;
    owner = 1;
  }
  std::string text;
  if (!args.get(2).isUndefined() && !ArgUTF8(cx, args.get(2), text)) return false;
  double n = 0;
  if (!JS::ToNumber(cx, args.get(3), &n)) return false;
  const char* out = nullptr;
  size_t len = 0;
  mrt_cssom_op(tJs->host, op, owner, id, text.data(), text.size(), n, &out, &len);
  return NewString(cx, out, len, args.rval());
}

// __mrtLinkSheet(link, url, css): a stylesheet <link>'s load ended, with the sheet's URL and text;
// __mrtLinkSheet(link, null): it has no sheet. The text goes to the runtime's linked-sheet cache,
// outside the arena (SPEC decision 13).
NATIVE(Global_mrtLinkSheet) {
  JS::CallArgs args = JS::CallArgsFromVp(argc, vp);
  uint32_t id;
  if (!ArgNode(cx, args, 0, &id)) return false;
  args.rval().setUndefined();
  if (args.get(1).isNull() || args.get(1).isUndefined()) {
    mrt_dom_link_sheet(tJs->host, id, nullptr, 0, nullptr, 0, 0);
    return true;
  }
  std::string url, css;
  if (!ArgUTF8(cx, args.get(1), url) || !ArgUTF8(cx, args.get(2), css)) return false;
  mrt_dom_link_sheet(tJs->host, id, url.data(), url.size(), css.data(), css.size(), 1);
  return true;
}

// __mrtParseDocument(html): DOMParser's parse of a text/html string, a new document.
NATIVE(Global_mrtParseDocument) {
  JS::CallArgs args = JS::CallArgsFromVp(argc, vp);
  std::string html;
  if (!ArgUTF8(cx, args.get(0), html)) return false;
  // with `true`, as a document whose scripts run (document.write's), else as DOMParser does
  if (JS::ToBoolean(args.get(1))) return Wrap(cx, mrt_dom_parse_frame_document(tJs->host, html.data(), html.size()), args.rval());
  return Wrap(cx, mrt_dom_parse_document(tJs->host, html.data(), html.size()), args.rval());
}

// __mrtCEReactions(fn, thisArg, ...args): a bridge.js method made of several DOM changes (innerHTML,
// replaceChildren, ...) run as one [CEReactions] method: its reactions run once, after all of it.
// __mrtLink(node, which): a shadow root's host (LINK_HOST), an element's shadow root (LINK_SHADOW_ROOT)
// or a node's assigned slot (LINK_ASSIGNED_SLOT), whatever the shadow root's mode, for event dispatch
// in prelude.js (Gecko's GetParentTarget sees closed trees too) and ResizeObserver's flattened-tree
// depth in bridge.js. null for anything else.
NATIVE(Global_mrtLink) {
  JS::CallArgs args = JS::CallArgsFromVp(argc, vp);
  uint32_t node = NodeOf(args.get(0));
  int32_t which = args.get(1).isInt32() ? args[1].toInt32() : -1;
  if (!node || (which != LINK_HOST && which != LINK_SHADOW_ROOT && which != LINK_ASSIGNED_SLOT)) {
    args.rval().setNull();
    return true;
  }
  return Wrap(cx, mrt_dom_link(tJs->host, node, which), args.rval());
}

NATIVE(Global_mrtCEReactions) {
  JS::CallArgs args = JS::CallArgsFromVp(argc, vp);
  AutoCEReaction ceReaction(cx);
  // restored before ceReaction runs the reactions (declared after it, so destroyed first)
  struct Restore {
    bool& flag;
    bool saved;
    ~Restore() { flag = saved; }
  } restore{tJs->ce.mInCompoundMethod, tJs->ce.mInCompoundMethod};
  tJs->ce.mInCompoundMethod = true;
  JS::RootedValueVector rest(cx);
  for (unsigned i = 2; i < args.length(); i++) {
    if (!rest.append(args[i])) return false;
  }
  return JS::Call(cx, args.get(1), args.get(0), rest, args.rval());
}

static const JSFunctionSpec kCustomElementRegistryFns[] = {
    JS_FN("define", CustomElementRegistry_define, 2, JSPROP_ENUMERATE),
    JS_FN("get", CustomElementRegistry_get, 1, JSPROP_ENUMERATE),
    JS_FN("getName", CustomElementRegistry_getName, 1, JSPROP_ENUMERATE),
    JS_FN("whenDefined", CustomElementRegistry_whenDefined, 1, JSPROP_ENUMERATE),
    JS_FN("upgrade", CustomElementRegistry_upgrade, 1, JSPROP_ENUMERATE),
    JS_FS_END};

// ---- console ----

// Gecko's FlushOutput (dom/console/Console.cpp): the text so far, if any, as one argument.
static bool FlushOutput(JSContext* aCx, JS::RootedValueVector& aSequence, std::string& aOutput) {
  if (!aOutput.empty()) {
    JS::RootedValue str(aCx);
    if (!NewString(aCx, aOutput.data(), aOutput.size(), &str) || !aSequence.append(str)) return false;
    aOutput.clear();
  }
  return true;
}

// Gecko's MakeFormatString: "%<integer>.<mantissa><ch>" for printf.
static std::string MakeFormatString(int32_t aInteger, int32_t aMantissa, char aCh) {
  std::string format = "%";
  if (aInteger >= 0) format += std::to_string(aInteger);
  if (aMantissa >= 0) format += "." + std::to_string(aMantissa);
  format += aCh;
  return format;
}

// Gecko's ProcessArguments (dom/console/Console.cpp): if there is more than one argument and the first
// is a non-empty string, it is a format string (%s, %d, %i, %f, %o, %O, %c, %%); what it does not use
// is appended. Styles (%c) take their argument and are dropped: the mini runtime's console is text.
// Works on UTF-8 bytes; '%' and digits are ASCII, so a multi-byte character is copied untouched.
static bool ProcessArguments(JSContext* aCx, const JS::CallArgs& aData, JS::RootedValueVector& aSequence) {
  if (aData.length() == 0) return true;
  auto appendAll = [&]() {
    for (unsigned i = 0; i < aData.length(); i++)
      if (!aSequence.append(aData[i])) return false;
    return true;
  };
  if (aData.length() == 1 || !aData[0].isString()) return appendAll();
  std::string string;
  if (!ArgUTF8(aCx, aData[0], string)) return false;
  if (string.empty()) return appendAll();

  std::string output;
  unsigned index = 1;
  size_t start = 0, end = string.size();
  while (start != end) {
    if (string[start] != '%') {
      output += string[start++];
      continue;
    }
    ++start;
    if (start == end) {
      output += '%';
      break;
    }
    if (string[start] == '%') {
      output += string[start++];
      continue;
    }
    std::string tmp = "%";
    int32_t integer = -1, mantissa = -1;
    // Let's parse %<number>.<number> for %d and %f
    if (string[start] >= '0' && string[start] <= '9') {
      integer = 0;
      do {
        integer = integer * 10 + string[start] - '0';
        tmp += string[start++];
      } while (start != end && string[start] >= '0' && string[start] <= '9');
    }
    if (start == end) {
      output += tmp;
      break;
    }
    if (string[start] == '.') {
      tmp += string[start++];
      if (start == end) {
        output += tmp;
        break;
      }
      // '.' must be followed by a number.
      if (string[start] < '0' || string[start] > '9') {
        output += tmp;
        continue;
      }
      mantissa = 0;
      do {
        mantissa = mantissa * 10 + string[start] - '0';
        tmp += string[start++];
      } while (start != end && string[start] >= '0' && string[start] <= '9');
      if (start == end) {
        output += tmp;
        break;
      }
    }
    char ch = string[start];
    tmp += ch;
    ++start;
    switch (ch) {
      case 'o':
      case 'O': {
        if (!FlushOutput(aCx, aSequence, output)) return false;
        JS::RootedValue v(aCx);
        if (index < aData.length()) v = aData[index++];
        if (!aSequence.append(v)) return false;
        break;
      }
      case 'c':
        if (!FlushOutput(aCx, aSequence, output)) return false;
        if (index < aData.length()) index++;  // the style, not shown in text
        break;
      case 's':
        if (index < aData.length()) {
          std::string v;
          if (!ArgUTF8(aCx, aData[index++], v)) return false;
          output += v;
        }
        break;
      case 'd':
      case 'i':
        if (index < aData.length()) {
          JS::RootedValue value(aCx, aData[index++]);
          if (value.isBigInt()) {
            std::string v;
            if (!ArgUTF8(aCx, value, v)) return false;
            output += v;
            break;
          }
          int32_t v;
          if (!JS::ToInt32(aCx, value, &v)) return false;
          char buf[64];
          snprintf(buf, sizeof(buf), MakeFormatString(integer, mantissa, 'd').c_str(), v);
          output += buf;
        }
        break;
      case 'f':
        if (index < aData.length()) {
          JS::RootedValue value(aCx, aData[index++]);
          double v;
          if (!JS::ToNumber(aCx, value, &v)) return false;
          if (std::isnan(v)) {
            output += "NaN";  // nspr returns "nan", but we want to expose it as "NaN"
          } else {
            char buf[512];
            snprintf(buf, sizeof(buf), MakeFormatString(integer, std::min(mantissa, 15), 'f').c_str(), v);
            output += buf;
          }
        }
        break;
      default:
        output += tmp;
        break;
    }
  }
  if (!FlushOutput(aCx, aSequence, output)) return false;
  // The rest of the array, if unused by the format string.
  for (; index < aData.length(); ++index)
    if (!aSequence.append(aData[index])) return false;
  return true;
}

// console.log and the others: the arguments after Gecko's format processing, each converted to a
// string and joined with spaces, as Firefox's stored console events read; an argument's stack on the
// lines after (as Firefox's console shows an Error's).
NATIVE(Console_log) {
  JS::CallArgs args = JS::CallArgsFromVp(argc, vp);
  JS::RootedValueVector values(cx);
  if (!ProcessArguments(cx, args, values)) return false;
  std::string line, part, stacks;
  for (size_t i = 0; i < values.length(); i++) {
    if (!ArgUTF8(cx, values[i], part)) return false;
    if (i) line += ' ';
    line += part;
    // any argument with a string `stack` (there is no public test for an Error object)
    if (values[i].isObject()) {
      JS::RootedObject obj(cx, &values[i].toObject());
      JS::RootedValue stack(cx);
      if (JS_GetProperty(cx, obj, "stack", &stack) && stack.isString() && ArgUTF8(cx, stack, part)) stacks += "\n" + part;
      JS_ClearPendingException(cx);
    }
  }
  line += stacks;
  mrt_console(tJs->host, line.data(), line.size());
  args.rval().setUndefined();
  return true;
}

// A promise rejected with no handler: its reason on the console, as Firefox's "Uncaught (in
// promise)". Printed when the rejection happens, so a handler added later (rare) still shows it.
// Gecko's CycleCollectedJSContext::PromiseRejectionTrackerCallback with PromiseDebugging's
// AddUncaughtRejection and AddConsumedRejection: an unhandled rejection is queued, not reported;
// if a handler is attached before the queue is flushed (FlushRejections, after the task's
// microtasks) it is taken off again. A promise handled after its unhandledrejection event gets a
// rejectionhandled event.
static void RejectionTracker(JSContext* cx, bool mutedErrors, JS::HandleObject promise,
                             JS::PromiseRejectionHandlingState state, void* data) {
  MrtJs* js = tJs;
  if (!js) return;
  auto remove = [&](std::vector<std::unique_ptr<JS::PersistentRootedObject>>& list) {
    for (auto& p : list)
      if (p && p->get() == promise) {
        p.reset();
        return true;
      }
    return false;
  };
  if (state == JS::PromiseRejectionHandlingState::Unhandled) {
    js->uncaught.push_back(std::make_unique<JS::PersistentRootedObject>(cx, promise));
    if (!mutedErrors) js->aboutToBeNotified.push_back(std::make_unique<JS::PersistentRootedObject>(cx, promise));
    return;
  }
  remove(js->uncaught);
  if (remove(js->aboutToBeNotified)) return;
  uint64_t id = JS::GetPromiseID(promise);
  for (auto& pending : js->pendingUnhandled)
    if (pending == id) {
      pending = 0;
      if (!mutedErrors) js->rejectionHandled.push_back(std::make_unique<JS::PersistentRootedObject>(cx, promise));
      return;
    }
}

// Calls __mrt.rejectionEvent(type, promise, reason) in the prelude, which dispatches a
// PromiseRejectionEvent at the window.
static void FireRejectionEvent(MrtJs* js, const char* type, JS::HandleObject promise) {
  JSContext* cx = js->cx;
  JS::RootedObject global(cx, *js->global);
  JS::RootedValue mrt(cx), fn(cx), rval(cx);
  if (!JS_GetProperty(cx, global, "__mrt", &mrt) || !mrt.isObject()) return;
  JS::RootedObject mrtObj(cx, &mrt.toObject());
  if (!JS_GetProperty(cx, mrtObj, "rejectionEvent", &fn) || !fn.isObject()) return;
  JS::RootedValueArray<3> args(cx);
  JSString* t = JS_NewStringCopyZ(cx, type);
  if (!t) return;
  args[0].setString(t);
  args[1].setObject(*promise);
  args[2].set(JS::GetPromiseResult(promise));
  if (!JS_CallFunctionValue(cx, mrtObj, fn, args, &rval)) JS_ClearPendingException(cx);
}

// Queues __mrtRejectionTask(kind) as a 0 ms task in the prelude's own timer queue
// (__mrt.queueTask), behind the timers already due: Gecko dispatches NotifyUnhandledRejections to
// the event loop, and a setTimeout(0) the script made runs before it.
static void QueueRejectionTask(MrtJs* js, int kind) {
  JSContext* cx = js->cx;
  JS::RootedObject global(cx, *js->global);
  JS::RootedValue mrt(cx), queue(cx), task(cx), rval(cx);
  if (!JS_GetProperty(cx, global, "__mrt", &mrt) || !mrt.isObject()) return;
  JS::RootedObject mrtObj(cx, &mrt.toObject());
  if (!JS_GetProperty(cx, mrtObj, "queueTask", &queue) || !queue.isObject() ||
      !JS_GetProperty(cx, global, "__mrtRejectionTask", &task)) {
    return;
  }
  JS::RootedValueArray<2> args(cx);
  args[0].set(task);
  args[1].setInt32(kind);
  if (!JS_CallFunctionValue(cx, mrtObj, queue, args, &rval)) JS_ClearPendingException(cx);
}

// At the end of a task, after its microtasks. Gecko's FlushRejections, which is dispatched when the
// first rejection is queued and so runs before anything the task queued: every rejection still
// uncaught is reported on the console (PromiseDebugging::FlushUncaughtRejections). The
// unhandledrejection events (NotifyUnhandledRejections, dispatched at AfterProcessMicrotasks) and
// rejectionhandled events become tasks of their own; whether a page cancels the event does not
// change the console report, as in Gecko.
static void FlushRejections(MrtJs* js) {
  JSContext* cx = js->cx;
  auto uncaught = std::move(js->uncaught);
  js->uncaught.clear();
  for (auto& p : uncaught) {
    if (!p) continue;
    JS::AutoSaveExceptionState saved(cx);
    JS::RootedValue reason(cx, JS::GetPromiseResult(*p));
    std::string line = "uncaught (in promise): ", part;
    if (ArgUTF8(cx, reason, part)) line += part;
    JS::RootedValue stack(cx);
    if (reason.isObject()) {
      JS::RootedObject obj(cx, &reason.toObject());
      if (JS_GetProperty(cx, obj, "stack", &stack) && stack.isString() && ArgUTF8(cx, stack, part)) line += "\n" + part;
    }
    JS_ClearPendingException(cx);
    mrt_console(js->host, line.data(), line.size());
  }
  auto notify = std::move(js->aboutToBeNotified);
  js->aboutToBeNotified.clear();
  if (std::any_of(notify.begin(), notify.end(), [](auto& p) { return bool(p); })) {
    js->notifyBatches.push_back(std::move(notify));
    QueueRejectionTask(js, 0);
  }
  if (!js->rejectionHandled.empty()) {
    js->handledBatches.push_back(std::move(js->rejectionHandled));
    js->rejectionHandled.clear();
    QueueRejectionTask(js, 1);
  }
}

static void MicrotaskCheckpoint(MrtJs* js);

// __mrtEnterCallback() and __mrtLeaveCallback(): around page code the glue calls (Gecko's
// nsAutoMicroTask and CallbackObject::CallSetup, EnterMicroTask and LeaveMicroTask). When the
// outermost one returns there is a microtask checkpoint (HTML's "clean up after running script").
NATIVE(Global_mrtEnterCallback) {
  JS::CallArgs args = JS::CallArgsFromVp(argc, vp);
  args.rval().setUndefined();
  tJs->callbackDepth++;
  return true;
}

// (not timed: the microtask checkpoint runs the page's scripts, whose own natives are timed)
static bool Global_mrtLeaveCallback(JSContext* cx, unsigned argc, JS::Value* vp) {
  JS::CallArgs args = JS::CallArgsFromVp(argc, vp);
  args.rval().setUndefined();
  MrtJs* js = tJs;
  if (js->callbackDepth > 0 && --js->callbackDepth == 0) MicrotaskCheckpoint(js);
  return true;
}

// __mrtRejectionTask(kind): the task FlushRejections queued. 0: NotifyUnhandledRejections::Run for
// the oldest batch, the cancelable unhandledrejection event for each promise still unhandled, which
// then waits for rejectionhandled; 1: the oldest batch of rejectionhandled events.
NATIVE(Global_mrtRejectionTask) {
  JS::CallArgs args = JS::CallArgsFromVp(argc, vp);
  args.rval().setUndefined();
  MrtJs* js = tJs;
  int kind = args.get(0).isInt32() ? args.get(0).toInt32() : 0;
  auto& batches = kind == 0 ? js->notifyBatches : js->handledBatches;
  if (batches.empty()) return true;
  auto batch = std::move(batches.front());
  batches.pop_front();
  for (auto& p : batch) {
    if (!p) continue;
    JS::RootedObject promise(cx, *p);
    if (kind == 1) {
      FireRejectionEvent(js, "rejectionhandled", promise);
      continue;
    }
    if (!JS::GetPromiseIsHandled(promise)) FireRejectionEvent(js, "unhandledrejection", promise);
    // still unhandled after the event: a later handler gets rejectionhandled
    if (!JS::GetPromiseIsHandled(promise)) js->pendingUnhandled.push_back(JS::GetPromiseID(promise));
  }
  js->pendingUnhandled.erase(std::remove(js->pendingUnhandled.begin(), js->pendingUnhandled.end(), 0),
                             js->pendingUnhandled.end());
  return true;
}

// __mrtStyle(op, cssText, name, value, important): mrt_style_op for the prelude's element.style;
// the result string, or null when a set or remove changed nothing.
NATIVE(Global_mrtStyle) {
  JS::CallArgs args = JS::CallArgsFromVp(argc, vp);
  int32_t op = 0;
  if (!JS::ToInt32(cx, args.get(0), &op)) return false;
  std::string css, name, value;
  if (!ArgUTF8(cx, args.get(1), css) || !ArgUTF8(cx, args.get(2), name) || !ArgUTF8(cx, args.get(3), value)) return false;
  const char* out = nullptr;
  size_t len = 0;
  int32_t r = mrt_style_op(tJs->host, op, css.data(), css.size(), name.data(), name.size(), value.data(), value.size(),
                           JS::ToBoolean(args.get(4)), &out, &len);
  if (!r) {
    args.rval().setNull();
    return true;
  }
  return NewString(cx, out, len, args.rval());
}

// __mrtComputed(op, element, pseudo, name): mrt_dom_computed_style for the prelude's getComputedStyle;
// the element is ignored for ops 1 and 2.
NATIVE(Global_mrtComputed) {
  JS::CallArgs args = JS::CallArgsFromVp(argc, vp);
  int32_t op = 0;
  if (!JS::ToInt32(cx, args.get(0), &op)) return false;
  uint32_t id = 0;
  if (op == 0 && !ArgNode(cx, args, 1, &id)) return false;
  std::string pseudo, name;
  if (!ArgUTF8(cx, args.get(2), pseudo) || !ArgUTF8(cx, args.get(3), name)) return false;
  const char* out = nullptr;
  size_t len = 0;
  mrt_dom_computed_style(tJs->host, id, op, pseudo.data(), pseudo.size(), name.data(), name.size(), &out, &len);
  return NewString(cx, out, len, args.rval());
}

// __mrtBridge(request): the request (a JSON string) to Firefox through the driver, the reply string
// back. The JS side (glue/bridge.js) makes and reads the JSON.
NATIVE(Global_mrtBridge) {
  JS::CallArgs args = JS::CallArgsFromVp(argc, vp);
  std::string req;
  if (!ArgUTF8(cx, args.get(0), req)) return false;
  const char* out = nullptr;
  size_t len = 0;
  if (!mrt_bridge(tJs->host, req.data(), req.size(), &out, &len)) {
    JS_ReportErrorUTF8(cx, "no bridge to Firefox");
    return false;
  }
  return NewString(cx, out, len, args.rval());
}

// ---- the tables ----

static const JSPropertySpec kNodeProps[] = {
    JS_PSG("parentNode", Node_parentNode, JSPROP_ENUMERATE),
    JS_PSG("parentElement", Node_parentElement, JSPROP_ENUMERATE),
    JS_PSG("firstChild", Node_firstChild, JSPROP_ENUMERATE),
    JS_PSG("lastChild", Node_lastChild, JSPROP_ENUMERATE),
    JS_PSG("nextSibling", Node_nextSibling, JSPROP_ENUMERATE),
    JS_PSG("previousSibling", Node_previousSibling, JSPROP_ENUMERATE),
    JS_PSG("nodeType", Node_nodeType, JSPROP_ENUMERATE),
    JS_PSG("nodeName", Node_nodeName, JSPROP_ENUMERATE),
    JS_PSGS("textContent", Node_textContent, Node_set_textContent, JSPROP_ENUMERATE),
    JS_PSG("childNodes", Node_childNodes, JSPROP_ENUMERATE),
    JS_PSG("ownerDocument", Node_ownerDocument, JSPROP_ENUMERATE),
    JS_PSG("isConnected", Node_isConnected, JSPROP_ENUMERATE),
    JS_PS_END};
static const JSPropertySpec kSlottableProps[] = {
    JS_PSG("assignedSlot", Slottable_assignedSlot, JSPROP_ENUMERATE), JS_PS_END};
static const JSFunctionSpec kNodeFns[] = {
    JS_FN("appendChild", Node_appendChild, 1, JSPROP_ENUMERATE),
    JS_FN("insertBefore", Node_insertBefore, 2, JSPROP_ENUMERATE),
    JS_FN("removeChild", Node_removeChild, 1, JSPROP_ENUMERATE),
    JS_FN("hasChildNodes", Node_hasChildNodes, 0, JSPROP_ENUMERATE),
    JS_FN("cloneNode", Node_cloneNode, 0, JSPROP_ENUMERATE),
    JS_FN("contains", Node_contains, 1, JSPROP_ENUMERATE),
    JS_FN("getRootNode", Node_getRootNode, 0, JSPROP_ENUMERATE),
    JS_FS_END};

// ParentNode members shared by Element and Document.
static const JSPropertySpec kParentNodeProps[] = {
    JS_PSG("children", Element_children, JSPROP_ENUMERATE),
    JS_PSG("firstElementChild", Element_firstElementChild, JSPROP_ENUMERATE),
    JS_PSG("lastElementChild", Element_lastElementChild, JSPROP_ENUMERATE),
    JS_PSG("childElementCount", Element_childElementCount, JSPROP_ENUMERATE),
    JS_PS_END};
static const JSFunctionSpec kParentNodeFns[] = {
    JS_FN("querySelector", ParentNode_querySelector, 1, JSPROP_ENUMERATE),
    JS_FN("querySelectorAll", ParentNode_querySelectorAll, 1, JSPROP_ENUMERATE),
    JS_FS_END};

static const JSPropertySpec kElementProps[] = {
    JS_PSG("tagName", Element_tagName, JSPROP_ENUMERATE),
    JS_PSG("localName", Element_localName, JSPROP_ENUMERATE),
    JS_PSG("namespaceURI", Element_namespaceURI, JSPROP_ENUMERATE),
    JS_PSGS("id", Element_id, Element_set_id, JSPROP_ENUMERATE),
    JS_PSGS("className", Element_className, Element_set_className, JSPROP_ENUMERATE),
    JS_PSG("nextElementSibling", Element_nextElementSibling, JSPROP_ENUMERATE),
    JS_PSG("previousElementSibling", Element_previousElementSibling, JSPROP_ENUMERATE),
    JS_PSG("shadowRoot", Element_shadowRoot, JSPROP_ENUMERATE),
    JS_PSG("assignedSlot", Slottable_assignedSlot, JSPROP_ENUMERATE),
    JS_PS_END};
static const JSFunctionSpec kElementFns[] = {
    JS_FN("attachShadow", Element_attachShadow, 1, JSPROP_ENUMERATE),
    JS_FN("getAttribute", Element_getAttribute, 1, JSPROP_ENUMERATE),
    JS_FN("getAttributeNames", Element_getAttributeNames, 0, JSPROP_ENUMERATE),
    JS_FN("setAttribute", Element_setAttribute, 2, JSPROP_ENUMERATE),
    JS_FN("removeAttribute", Element_removeAttribute, 1, JSPROP_ENUMERATE),
    JS_FN("hasAttribute", Element_hasAttribute, 1, JSPROP_ENUMERATE),
    JS_FN("hasAttributes", Element_hasAttributes, 0, JSPROP_ENUMERATE),
    JS_FN("matches", Element_matches, 1, JSPROP_ENUMERATE),
    JS_FN("webkitMatchesSelector", Element_matches, 1, JSPROP_ENUMERATE),
    JS_FN("closest", Element_closest, 1, JSPROP_ENUMERATE),
    JS_FN("getBoundingClientRect", Element_getBoundingClientRect, 0, JSPROP_ENUMERATE),
    JS_FS_END};
// HTMLElement: the offset* members.
static const JSPropertySpec kHTMLElementProps[] = {
    JS_PSG("offsetParent", Element_offsetParent, JSPROP_ENUMERATE),
    JS_PSG("offsetLeft", Element_offsetLeft, JSPROP_ENUMERATE),
    JS_PSG("offsetTop", Element_offsetTop, JSPROP_ENUMERATE),
    JS_PSG("offsetWidth", Element_offsetWidth, JSPROP_ENUMERATE),
    JS_PSG("offsetHeight", Element_offsetHeight, JSPROP_ENUMERATE),
    JS_PS_END};

static const JSPropertySpec kCharacterDataProps[] = {
    JS_PSGS("data", Node_textContent, Node_set_textContent, JSPROP_ENUMERATE),
    JS_PSG("length", CharacterData_length, JSPROP_ENUMERATE),
    JS_PSG("nextElementSibling", Element_nextElementSibling, JSPROP_ENUMERATE),
    JS_PSG("previousElementSibling", Element_previousElementSibling, JSPROP_ENUMERATE),
    JS_PS_END};

static const JSPropertySpec kDocumentProps[] = {
    JS_PSG("documentElement", Document_documentElement, JSPROP_ENUMERATE),
    JS_PSG("body", Document_body, JSPROP_ENUMERATE),
    JS_PSG("head", Document_head, JSPROP_ENUMERATE),
    JS_PSG("doctype", Document_doctype, JSPROP_ENUMERATE),
    JS_PSG("compatMode", Document_compatMode, JSPROP_ENUMERATE),
    JS_PS_END};
static const JSPropertySpec kDocumentTypeProps[] = {
    JS_PSG("name", DocumentType_name, JSPROP_ENUMERATE),
    JS_PSG("publicId", DocumentType_emptyId, JSPROP_ENUMERATE),
    JS_PSG("systemId", DocumentType_emptyId, JSPROP_ENUMERATE),
    JS_PS_END};
static const JSFunctionSpec kDocumentFns[] = {
    JS_FN("getElementById", Document_getElementById, 1, JSPROP_ENUMERATE),
    JS_FN("createElement", Document_createElement, 1, JSPROP_ENUMERATE),
    JS_FN("createElementNS", Document_createElementNS, 2, JSPROP_ENUMERATE),
    JS_FN("createTextNode", Document_createTextNode, 1, JSPROP_ENUMERATE),
    JS_FN("createComment", Document_createComment, 1, JSPROP_ENUMERATE),
    JS_FN("createDocumentFragment", Document_createDocumentFragment, 0, JSPROP_ENUMERATE),
    JS_FN("importNode", Document_importNode, 1, JSPROP_ENUMERATE),
    JS_FN("adoptNode", Document_adoptNode, 1, JSPROP_ENUMERATE),
    JS_FS_END};

static const JSFunctionSpec kConsoleFns[] = {
    JS_FN("log", Console_log, 0, JSPROP_ENUMERATE),
    JS_FN("info", Console_log, 0, JSPROP_ENUMERATE),
    JS_FN("warn", Console_log, 0, JSPROP_ENUMERATE),
    JS_FN("error", Console_log, 0, JSPROP_ENUMERATE),
    JS_FS_END};

// ---- module scripts (js/loader/ModuleLoaderBase.cpp, dom/script/ModuleLoader.cpp) ----
//
// Gecko's module loader, reduced to one global and to fetches that complete at once. Gecko fetches
// each module on the network and resumes the graph's loading when the fetch completes; the mini
// runtime's fetches go through the bridge, which blocks. So a static import is fetched and compiled
// inside HostLoadImportedModule and a graph finishes loading inside LoadRequestedModules (its
// callback form, which Gecko uses for chrome modules and workers). A dynamic import() still
// completes in a later task, as Gecko's does from the network: the fetch, the graph and
// FinishLoadingImportedModule run as a 0 ms task (__mrt.queueTask). The script element's side
// (when a module script runs, its load and error events) is in prelude.js; fetching and URL parsing
// are __mrt.fetchModule and __mrt.resolveURL there, the bridge's versions in bridge.js.
// No import maps, no module preloading, no SRI; JavaScript and JSON modules only.

static std::string ModuleMapKey(const std::string& aURI, JS::ModuleType aType) {
  return std::to_string(uint32_t(aType)) + " " + aURI;
}

// ModuleLoadRequest, the part a request needs while it is handled: what kind of load it is, its
// URL and type, and the module script once fetched.
struct ModuleLoadRequest {
  enum class Kind { TopLevel, StaticImport, DynamicImport };
  Kind mKind;
  std::string mURI;
  JS::ModuleType mModuleType = JS::ModuleType::JavaScript;
  ModuleScript* mModuleScript = nullptr;
  bool mErrored = false;
  bool IsTopLevel() const { return mKind == Kind::TopLevel; }
  bool IsDynamicImport() const { return mKind == Kind::DynamicImport; }
};

// Calls __mrt[aName](args) (prelude.js or bridge.js).
static bool CallMrtHook(JSContext* cx, const char* aName, const JS::HandleValueArray& aArgs,
                        JS::MutableHandleValue aRval) {
  JS::RootedObject global(cx, JS::CurrentGlobalOrNull(cx));
  JS::RootedValue mrt(cx);
  if (!global || !JS_GetProperty(cx, global, "__mrt", &mrt) || !mrt.isObject()) return false;
  JS::RootedObject mrtObj(cx, &mrt.toObject());
  return JS_CallFunctionName(cx, mrtObj, aName, aArgs, aRval);
}

// The document's base URL (__mrt.baseURL): the referrer of an import from a classic script.
static std::string DocumentBaseURL(JSContext* cx) {
  JS::RootedValue rval(cx);
  std::string out;
  if (!CallMrtHook(cx, "baseURL", JS::HandleValueArray::empty(), &rval) || !ArgUTF8(cx, rval, out)) {
    JS_ClearPendingException(cx);
    return "about:blank";
  }
  return out;
}

// GetScriptFetchInfoOrNull: a module's fetch info from its private. Classic scripts have none (Gecko
// gives each its own, whose base URL is an external script's URL; here an import() from a classic
// script resolves against the document's base URL).
static ScriptFetchInfo* GetScriptFetchInfoOrNull(JSScript* aScript) {
  if (!aScript) return nullptr;
  JS::Value priv = JS::GetScriptPrivate(aScript);
  return priv.isUndefined() ? nullptr : static_cast<ScriptFetchInfo*>(priv.toPrivate());
}

// ResolveError (js/loader/ResolveResult.h), without the import map ones.
enum class ResolveError { None, Failure, InvalidBareSpecifier };

// ImportMap::ResolveModuleSpecifier with no import map, that is ResolveURLLikeModuleSpecifier: a
// specifier starting with "/", "./" or "../" is parsed against the base URL, any other one as an
// absolute URL; one that is neither is a bare specifier (InvalidBareSpecifier). __mrt.resolveURL
// (spec, base) answers null for a failure (base null: absolute only).
static ResolveError ResolveModuleSpecifier(JSContext* cx, const std::string& aBaseURL, const std::string& aSpecifier,
                                           std::string& aOut) {
  auto startsWith = [&](const char* p) { return aSpecifier.compare(0, strlen(p), p) == 0; };
  bool relative = startsWith("/") || startsWith("./") || startsWith("../");
  JS::RootedValueArray<2> args(cx);
  JS::RootedValue rval(cx);
  if (!NewString(cx, aSpecifier.data(), aSpecifier.size(), args[0])) return ResolveError::Failure;
  if (relative) {
    if (!NewString(cx, aBaseURL.data(), aBaseURL.size(), args[1])) return ResolveError::Failure;
  } else {
    args[1].setNull();
  }
  if (!CallMrtHook(cx, "resolveURL", args, &rval) || !rval.isString() || !ArgUTF8(cx, rval, aOut)) {
    JS_ClearPendingException(cx);
    return relative ? ResolveError::Failure : ResolveError::InvalidBareSpecifier;
  }
  return ResolveError::None;
}

// HandleResolveFailure: the TypeError, with dom.properties' messages (ResolveErrorInfo::GetString).
static void HandleResolveFailure(JSContext* cx, ScriptFetchInfo* aFetchInfo, const std::string& aSpecifier,
                                 ResolveError aError, uint32_t aLineNumber, JS::ColumnNumberOneOrigin aColumnNumber,
                                 JS::MutableHandleValue aErrorOut) {
  std::string filename = aFetchInfo ? aFetchInfo->mBaseURL : "(unknown)";
  // ModuleResolveFailureNoWarn and ImportMapResolveInvalidBareSpecifierWarnRelative
  std::string text = aError == ResolveError::InvalidBareSpecifier
                         ? "The specifier \xe2\x80\x9c" + aSpecifier +
                               "\xe2\x80\x9d was a bare specifier, but was not remapped to anything. Relative module "
                               "specifiers must start with \xe2\x80\x9c./\xe2\x80\x9d, \xe2\x80\x9c../\xe2\x80\x9d or "
                               "\xe2\x80\x9c/\xe2\x80\x9d."
                         : "Error resolving module specifier \xe2\x80\x9c" + aSpecifier + "\xe2\x80\x9d.";
  JS::RootedValue file(cx), message(cx);
  bool ok = NewString(cx, filename.data(), filename.size(), &file) && NewString(cx, text.data(), text.size(), &message);
  JS::RootedString fileString(cx, ok ? file.toString() : nullptr), messageString(cx, ok ? message.toString() : nullptr);
  if (!ok || !JS::CreateError(cx, JSEXN_TYPEERR, nullptr, fileString, aLineNumber, aColumnNumber, nullptr,
                              messageString, JS::NothingHandleValue, aErrorOut)) {
    JS_ClearPendingException(cx);
    aErrorOut.setUndefined();
  }
}

// ScriptLoader::ReportErrorToConsole for a module whose fetch failed (ModuleSourceLoadFailed).
static void ReportModuleFetchFailure(const std::string& aURI) {
  tJs->modules.mFetchFailures++;
  std::string line = "Loading failed for the module with source \xe2\x80\x9c" + aURI + "\xe2\x80\x9d.";
  mrt_console(tJs->host, line.data(), line.size());
}

// The module's source text: __mrt.fetchModule(url, type) gives {url, text} (the final URL after
// redirects), or null when the fetch failed, the status was not 2xx or the MIME type is not the
// module type's (MimeTypeMatchesExpectedModuleType). Gecko's StartFetch through OnStreamComplete,
// which reports every failed fetch on the console.
static bool FetchModuleSource(JSContext* cx, ModuleLoadRequest* aRequest, std::string& aBaseURL,
                              JS::MutableHandleString aText) {
  tJs->modules.mFetches++;
  JS::RootedValueArray<2> args(cx);
  JS::RootedValue rval(cx);
  if (!NewString(cx, aRequest->mURI.data(), aRequest->mURI.size(), args[0])) return false;
  JSString* type = JS_NewStringCopyZ(cx, aRequest->mModuleType == JS::ModuleType::JSON ? "json" : "javascript");
  if (!type) return false;
  args[1].setString(type);
  if (!CallMrtHook(cx, "fetchModule", args, &rval) || !rval.isObject()) {
    JS_ClearPendingException(cx);
    ReportModuleFetchFailure(aRequest->mURI);
    return false;
  }
  JS::RootedObject result(cx, &rval.toObject());
  JS::RootedValue url(cx), text(cx);
  if (!JS_GetProperty(cx, result, "url", &url) || !JS_GetProperty(cx, result, "text", &text) || !text.isString() ||
      !ArgUTF8(cx, url, aBaseURL)) {
    JS_ClearPendingException(cx);
    ReportModuleFetchFailure(aRequest->mURI);
    return false;
  }
  aText.set(text.toString());
  return true;
}

// CreateModuleScript: compiles the source as a module (JSON for a JSON module); a compile error
// becomes the module script's parse error. The record's private is the fetch info.
static ModuleScript* CreateModuleScript(JSContext* cx, std::unique_ptr<ModuleScript>& aOut, JS::HandleString aText,
                                        const std::string& aBaseURL, const std::string& aFilename,
                                        JS::ModuleType aType) {
  aOut = std::make_unique<ModuleScript>();
  ModuleScript* ms = aOut.get();
  ms->mFetchInfo.mBaseURL = aBaseURL;
  JS::CompileOptions options(cx);
  options.setFileAndLine(aFilename.c_str(), 1).setNoScriptRval(true).setIntroductionType("srcScript");
  JS::RootedObject module(cx);
  {
    JS::AutoStableStringChars chars(cx);
    JS::SourceText<char16_t> source;
    if (chars.initTwoByte(cx, aText) &&
        source.init(cx, chars.twoByteChars(), JS_GetStringLength(aText), JS::SourceOwnership::Borrowed)) {
      module = aType == JS::ModuleType::JSON ? JS::CompileJsonModule(cx, options, source)
                                             : JS::CompileModule(cx, options, source);
    }
  }
  if (!module) {
    JS::RootedValue error(cx);
    if (!JS_GetPendingException(cx, &error) || error.isUndefined()) {
      JS_ClearPendingException(cx);
      aOut.reset();
      return nullptr;
    }
    JS_ClearPendingException(cx);
    ms->mParseError = error;
    return ms;
  }
  ms->mModuleRecord = module;
  // a JSON module is a synthetic module: no script, no private
  if (JS::IsCyclicModule(module)) JS::SetModulePrivate(module, JS::PrivateValue(&ms->mFetchInfo));
  return ms;
}

static bool OnLoadRequestedModulesResolved(JSContext* cx, JS::HandleValue aHostDefined);
static bool OnLoadRequestedModulesRejected(JSContext* cx, JS::HandleValue aHostDefined, JS::HandleValue aError);

// StartFetchingModuleDependencies: loads the graph below a top-level module or a dynamic import's
// module. Every fetch completes at once, so the graph is loaded (or failed) when this returns.
static void StartFetchingModuleDependencies(JSContext* cx, ModuleLoadRequest* aRequest) {
  JS::RootedObject module(cx, aRequest->mModuleScript->mModuleRecord);
  JS::RootedValue hostDefined(cx, JS::PrivateValue(aRequest));
  if (!JS::LoadRequestedModules(cx, module, hostDefined, OnLoadRequestedModulesResolved,
                                OnLoadRequestedModulesRejected)) {
    JS::RootedValue error(cx);
    if (!JS_GetPendingException(cx, &error)) error.setUndefined();
    JS_ClearPendingException(cx);
    OnLoadRequestedModulesRejected(cx, hostDefined, error);
  }
  // the dynamic import's error, if any, stays pending for DynamicImportTask; a top-level one is in
  // the module script
}

static bool OnLoadRequestedModulesResolved(JSContext* cx, JS::HandleValue aHostDefined) {
  // the request is ready (SetReady, LoadFinished); its caller goes on from there
  return true;
}

static bool OnLoadRequestedModulesRejected(JSContext* cx, JS::HandleValue aHostDefined, JS::HandleValue aError) {
  auto* request = static_cast<ModuleLoadRequest*>(aHostDefined.toPrivate());
  ModuleScript* moduleScript = request->mModuleScript;
  if (request->IsDynamicImport()) {
    // the import's promise is rejected by DynamicImportTask with this error, left pending for it
    // (as the shell's loader does), or with Gecko's TypeError when there is none
    if (!aError.isUndefined()) JS_SetPendingException(cx, aError);
  } else if (moduleScript && !aError.isUndefined()) {
    // Step 7.1. If state.[[ErrorToRethrow]] is not null, set moduleScript's error to rethrow
    moduleScript->mErrorToRethrow = aError;
  } else {
    // Step 7.2. Otherwise, run onComplete given null.
    request->mModuleScript = nullptr;
  }
  request->mErrored = true;
  return true;
}

// StartModuleLoad, through OnFetchComplete: a module in the map (fetched before, or a failed fetch)
// is used as it is (WaitForModuleFetch, then ResumeWaitingRequest); otherwise it is fetched and
// compiled and put in the map. Afterwards mModuleScript is null when the fetch failed.
static void StartModuleLoad(JSContext* cx, ModuleLoadRequest* aRequest) {
  ModuleLoader& loader = tJs->modules;
  std::string key = ModuleMapKey(aRequest->mURI, aRequest->mModuleType);
  auto it = loader.mFetchedModules.find(key);
  if (it == loader.mFetchedModules.end()) {
    std::string baseURL;
    JS::RootedString text(cx);
    std::unique_ptr<ModuleScript> ms;
    if (FetchModuleSource(cx, aRequest, baseURL, &text)) {
      CreateModuleScript(cx, ms, text, baseURL, aRequest->mURI, aRequest->mModuleType);
    }
    // SetModuleFetchFinishedAndGetWaitingRequests: the result goes into the map, failed or not
    it = loader.mFetchedModules.emplace(key, std::move(ms)).first;
  }
  aRequest->mModuleScript = it->second.get();
  if (!aRequest->mModuleScript || aRequest->mModuleScript->HasParseError()) aRequest->mErrored = true;
}

// OnFetchSucceeded and OnFetchFailed for a static import: completes the importing module's request
// for it (FinishLoadingImportedModule), with the module or with the error.
static bool FinishStaticImport(JSContext* cx, ModuleLoadRequest* aRequest, JS::HandleScript aReferrer,
                               JS::HandleObject aModuleRequest, JS::HandleValue aPayload) {
  ModuleScript* ms = aRequest->mModuleScript;
  if (!ms) {
    // Step 14.2: a null module script; the TypeError is not made, as nothing reads it for a
    // static import (OnFetchFailed)
    return JS::FinishLoadingImportedModuleFailed(cx, aPayload, JS::UndefinedHandleValue);
  }
  if (ms->HasParseError()) {
    JS::RootedValue parseError(cx, ms->mParseError);
    return JS::FinishLoadingImportedModuleFailed(cx, aPayload, parseError);
  }
  JS::RootedObject module(cx, ms->mModuleRecord);
  return JS::FinishLoadingImportedModule(cx, aReferrer, aModuleRequest, aPayload, module, false);
}

static bool DynamicImportTask(JSContext* cx, unsigned argc, JS::Value* vp);

// ModuleLoaderBase::HostLoadImportedModule (https://html.spec.whatwg.org/#hostloadimportedmodule).
static bool HostLoadImportedModule(JSContext* cx, JS::HandleScript aReferrer, JS::HandleObject aModuleRequest,
                                   JS::HandleValue aHostDefined, JS::HandleValue aPayload, uint32_t aLineNumber,
                                   JS::ColumnNumberOneOrigin aColumnNumber) {
  JS::RootedObject payloadObject(cx, aPayload.isObject() ? &aPayload.toObject() : nullptr);
  bool isDynamicImport = payloadObject && JS::IsPromiseObject(payloadObject);
  JS::RootedString specifierString(cx, JS::GetModuleRequestSpecifier(cx, aModuleRequest));
  std::string specifier;
  JS::RootedValue spec(cx, JS::StringValue(specifierString));
  if (!specifierString || !ArgUTF8(cx, spec, specifier)) return false;

  ScriptFetchInfo* fetchInfo = GetScriptFetchInfoOrNull(aReferrer);
  std::string baseURL = fetchInfo ? fetchInfo->mBaseURL : DocumentBaseURL(cx);
  std::string url;
  ResolveError error = ResolveModuleSpecifier(cx, baseURL, specifier, url);
  // Step 9. If the previous step threw an exception: FinishLoadingImportedModule with it
  if (error != ResolveError::None) {
    JS::RootedValue errorValue(cx);
    HandleResolveFailure(cx, fetchInfo, specifier, error, aLineNumber, aColumnNumber, &errorValue);
    JS::FinishLoadingImportedModuleFailed(cx, aPayload, errorValue);
    return true;
  }

  JS::ModuleType moduleType = JS::GetModuleRequestType(cx, aModuleRequest);
  // IsModuleTypeAllowed; CSS modules are behind a pref in Firefox and not built here
  if (moduleType != JS::ModuleType::JavaScript && moduleType != JS::ModuleType::JSON) {
    std::string text = url + ": invalid module type";
    std::string filename = fetchInfo ? fetchInfo->mBaseURL : "(unknown)";
    JS::RootedValue file(cx), message(cx), errorValue(cx);
    bool ok = NewString(cx, filename.data(), filename.size(), &file) && NewString(cx, text.data(), text.size(), &message);
    JS::RootedString fileString(cx, ok ? file.toString() : nullptr), messageString(cx, ok ? message.toString() : nullptr);
    if (ok && JS::CreateError(cx, JSEXN_TYPEERR, nullptr, fileString, 0, JS::ColumnNumberOneOrigin(), nullptr,
                              messageString, JS::NothingHandleValue, &errorValue)) {
      JS_SetPendingException(cx, errorValue);
    }
    return false;
  }

  if (isDynamicImport) {
    // the rest of the load is a task of its own (Gecko's fetch completes from the network)
    // (no referrer: it only records the module on an importing module, which the module map does
    // here too; the shell's loader passes none either)
    JS::RootedObject closure(cx, JS_NewPlainObject(cx));
    JS::RootedValue request(cx, JS::ObjectValue(*aModuleRequest)), urlValue(cx);
    if (!closure || !NewString(cx, url.data(), url.size(), &urlValue) ||
        !JS_DefineProperty(cx, closure, "moduleRequest", request, 0) ||
        !JS_DefineProperty(cx, closure, "payload", aPayload, 0) || !JS_DefineProperty(cx, closure, "url", urlValue, 0)) {
      return false;
    }
    JSFunction* fun = JS_NewFunction(cx, DynamicImportTask, 1, 0, "DynamicImportTask");
    if (!fun) return false;
    JS::RootedValueArray<2> args(cx);
    args[0].setObject(*JS_GetFunctionObject(fun));
    args[1].setObject(*closure);
    JS::RootedValue rval(cx);
    if (!CallMrtHook(cx, "queueTask", args, &rval)) {
      JS_ReportErrorNumberASCII(cx, js::GetErrorMessage, nullptr, JSMSG_DYNAMIC_IMPORT_FAILED, url.c_str());
      return false;
    }
    return true;
  }

  ModuleLoadRequest request{ModuleLoadRequest::Kind::StaticImport, url, moduleType};
  StartModuleLoad(cx, &request);
  return FinishStaticImport(cx, &request, aReferrer, aModuleRequest, aPayload);
}

// The task of a dynamic import: StartModuleLoad, StartFetchingModuleDependencies (OnFetchSucceeded
// for a dynamic import), then ProcessDynamicImport, which hands the module to the engine to link and
// evaluate in a promise job (usePromise, as Gecko's script loader); or the import's promise is
// rejected (OnFetchFailed, OnLoadRequestedModulesRejected).
static bool DynamicImportTask(JSContext* cx, unsigned argc, JS::Value* vp) {
  JS::CallArgs args = JS::CallArgsFromVp(argc, vp);
  args.rval().setUndefined();
  if (!args.get(0).isObject()) return true;
  JS::RootedObject closure(cx, &args[0].toObject());
  JS::RootedValue requestValue(cx), payload(cx), urlValue(cx);
  std::string url;
  if (!JS_GetProperty(cx, closure, "moduleRequest", &requestValue) || !JS_GetProperty(cx, closure, "payload", &payload) ||
      !JS_GetProperty(cx, closure, "url", &urlValue) || !ArgUTF8(cx, urlValue, url)) {
    return false;
  }
  JS::RootedObject moduleRequest(cx, &requestValue.toObject());
  ModuleLoadRequest request{ModuleLoadRequest::Kind::DynamicImport, url,
                            JS::GetModuleRequestType(cx, moduleRequest)};
  StartModuleLoad(cx, &request);
  ModuleScript* ms = request.mModuleScript;
  if (ms && ms->HasParseError()) {
    JS::RootedValue parseError(cx, ms->mParseError);
    return JS::FinishLoadingImportedModuleFailed(cx, payload, parseError);
  }
  JS::RootedValue loadError(cx);
  if (ms) {
    StartFetchingModuleDependencies(cx, &request);
    if (request.mErrored && JS_IsExceptionPending(cx)) {
      JS_GetPendingException(cx, &loadError);
      JS_ClearPendingException(cx);
    }
  }
  if (!ms || request.mErrored) {
    if (!loadError.isUndefined()) return JS::FinishLoadingImportedModuleFailed(cx, payload, loadError);
    JS_ReportErrorNumberASCII(cx, js::GetErrorMessage, nullptr, JSMSG_DYNAMIC_IMPORT_FAILED, url.c_str());
    return JS::FinishLoadingImportedModuleFailedWithPendingException(cx, payload);
  }
  // ProcessDynamicImport, then FinishLoadingImportedModule
  JS::RootedObject module(cx, ms->mModuleRecord);
  return JS::FinishLoadingImportedModule(cx, nullptr, moduleRequest, payload, module, true);
}

// import.meta.resolve (ModuleLoaderBase::ImportMetaResolve): the specifier resolved against the
// module's base URL; the module record is in the function's reserved slot.
static bool ImportMetaResolve(JSContext* cx, unsigned argc, JS::Value* vp) {
  JS::CallArgs args = JS::CallArgsFromVp(argc, vp);
  JS::RootedObject moduleRecord(cx, &js::GetFunctionNativeReserved(&args.callee(), 0).toObject());
  auto* fetchInfo = static_cast<ScriptFetchInfo*>(JS::GetModulePrivate(moduleRecord).toPrivate());
  std::string specifier, url;
  if (!ArgUTF8(cx, args.get(0), specifier)) return false;
  ResolveError error = ResolveModuleSpecifier(cx, fetchInfo->mBaseURL, specifier, url);
  if (error != ResolveError::None) {
    JS::RootedValue errorValue(cx);
    HandleResolveFailure(cx, fetchInfo, specifier, error, 0, JS::ColumnNumberOneOrigin(), &errorValue);
    JS_SetPendingException(cx, errorValue);
    return false;
  }
  return NewString(cx, url.data(), url.size(), args.rval());
}

// HostPopulateImportMeta: import.meta.url, the module's base URL, and import.meta.resolve.
static bool HostPopulateImportMeta(JSContext* cx, JS::HandleObject aModuleRecord, JS::HandleObject aMetaObject) {
  auto* fetchInfo = static_cast<ScriptFetchInfo*>(JS::GetModulePrivate(aModuleRecord).toPrivate());
  JS::RootedValue url(cx);
  if (!NewString(cx, fetchInfo->mBaseURL.data(), fetchInfo->mBaseURL.size(), &url) ||
      !JS_DefineProperty(cx, aMetaObject, "url", url, JSPROP_ENUMERATE)) {
    return false;
  }
  JSFunction* resolveFunc = js::DefineFunctionWithReserved(cx, aMetaObject, "resolve", ImportMetaResolve, 1,
                                                           JSPROP_ENUMERATE);
  if (!resolveFunc) return false;
  js::SetFunctionNativeReserved(JS_GetFunctionObject(resolveFunc), 0, JS::ObjectValue(*aModuleRecord));
  return true;
}

// An exception the engine reports by itself: a module whose evaluation threw (OnRootModuleRejected)
// or an uncatchable job. Gecko's preparer reports it through an AutoEntryScript; here on the console.
static void ReportModuleException(JSContext* cx) {
  JS::RootedValue exn(cx);
  if (!JS_GetPendingException(cx, &exn)) {
    JS_ClearPendingException(cx);
    return;
  }
  JS_ClearPendingException(cx);
  // in a worker, the worker's error report (bridge.js: an ErrorEvent at its global, then its parent)
  if (tJs->worker) {
    JS::RootedObject global(cx, JS::CurrentGlobalOrNull(cx));
    JS::RootedValue mrt(cx), fn(cx), rval(cx);
    if (global && JS_GetProperty(cx, global, "__mrt", &mrt) && mrt.isObject()) {
      JS::RootedObject mrtObj(cx, &mrt.toObject());
      if (JS_GetProperty(cx, mrtObj, "workerError", &fn) && fn.isObject() &&
          JS_CallFunctionValue(cx, mrtObj, fn, JS::HandleValueArray(exn), &rval)) {
        return;
      }
    }
    JS_ClearPendingException(cx);
  }
  std::string line = "uncaught exception in a module script: ", part;
  if (ArgUTF8(cx, exn, part)) line += part;
  JS::RootedValue stack(cx);
  if (exn.isObject()) {
    JS::RootedObject obj(cx, &exn.toObject());
    if (JS_GetProperty(cx, obj, "stack", &stack) && stack.isString() && ArgUTF8(cx, stack, part)) line += "\n" + part;
  }
  JS_ClearPendingException(cx);
  mrt_console(tJs->host, line.data(), line.size());
}

// CycleCollectedJSRuntime::EnvironmentPreparer
struct EnvironmentPreparer final : js::ScriptEnvironmentPreparer {
  void invoke(JS::HandleObject global, Closure& closure) override {
    JSContext* cx = tJs->cx;
    JSAutoRealm ar(cx, global);
    if (!closure(cx) && JS_IsExceptionPending(cx)) ReportModuleException(cx);
  }
};
static EnvironmentPreparer sEnvironmentPreparer;

// __mrtModuleScript(url, text): a <script type=module>'s module graph fetched, linked and evaluated,
// as ScriptLoader and ModuleLoaderBase do for a top-level module request. `text` null: an external
// script at `url`; otherwise an inline one whose base URL is `url`. Returns 0 when evaluated, 1 when
// the script or a module of its graph could not be fetched (the element's error event), 2 when it
// threw while compiling, linking or evaluating (reported on the console). In a module worker (no
// element; `text` null) a graph that did not compile or link returns 3 with nothing reported, and an
// evaluation error is reported through the worker's error report (ReportModuleException).
NATIVE(Global_mrtModuleScript) {
  JS::CallArgs args = JS::CallArgsFromVp(argc, vp);
  std::string url;
  if (!ArgUTF8(cx, args.get(0), url)) return false;
  ModuleLoadRequest request{ModuleLoadRequest::Kind::TopLevel, url};
  ModuleLoader& loader = tJs->modules;
  if (args.get(1).isString()) {
    JS::RootedString text(cx, args[1].toString());
    std::unique_ptr<ModuleScript> ms;
    request.mModuleScript = CreateModuleScript(cx, ms, text, url, url, JS::ModuleType::JavaScript);
    if (ms) loader.mInline.push_back(std::move(ms));
    if (!request.mModuleScript || request.mModuleScript->HasParseError()) request.mErrored = true;
  } else {
    StartModuleLoad(cx, &request);
  }
  ModuleScript* ms = request.mModuleScript;
  if (ms && ms->HasParseError()) {
    // OnFetchFailed for a top-level request: the parse error is the error to rethrow
    ms->mErrorToRethrow = ms->mParseError;
  } else if (ms && !ms->HasErrorToRethrow()) {
    StartFetchingModuleDependencies(cx, &request);
  }
  ms = request.mModuleScript;
  if (!ms) {
    // the error event (the failed fetch was reported on the console)
    args.rval().setInt32(1);
    return true;
  }
  // InstantiateModuleGraph
  JS::RootedObject module(cx, ms->mModuleRecord);
  if (!ms->HasErrorToRethrow() && !JS::ModuleLink(cx, module)) {
    JS::RootedValue exception(cx);
    if (JS_GetPendingException(cx, &exception)) ms->mErrorToRethrow = exception;
    JS_ClearPendingException(cx);
  }
  // EvaluateModule
  if (ms->HasErrorToRethrow()) {
    // a module worker's graph that did not parse, load or link fails the worker's load (a plain error
    // event at the Worker, WorkerScriptLoader), with nothing reported
    if (tJs->worker) {
      args.rval().setInt32(3);
      return true;
    }
    JS::RootedValue error(cx, ms->mErrorToRethrow);
    JS_SetPendingException(cx, error);
    ReportModuleException(cx);
    args.rval().setInt32(2);
    return true;
  }
  JS::RootedValue rval(cx);
  bool ok = JS::ModuleEvaluate(cx, module, &rval);
  JS::RootedObject evaluationPromise(cx, rval.isObject() ? &rval.toObject() : nullptr);
  if (!ok) {
    ReportModuleException(cx);
    args.rval().setInt32(2);
    return true;
  }
  bool threw = evaluationPromise && JS::GetPromiseState(evaluationPromise) == JS::PromiseState::Rejected;
  // the rejection is reported by the engine (OnRootModuleRejected, through the preparer)
  if (!JS::ThrowOnModuleEvaluationFailure(cx, evaluationPromise, JS::ReportModuleErrorsAsync)) {
    ReportModuleException(cx);
  }
  args.rval().setInt32(threw ? 2 : 0);
  return true;
}

// __mrtClassicScript(text, url, line, column[, callerURL, callerLine]): a window's classic script
// compiled and run in this global, as ScriptLoader's FillCompileOptionsForRequest and EvaluateScript
// do: named `url` (the script's own for an external one, the document's for an inline one) from
// `line`, and from `column` (one-based) when it is not 0, which is a parser-made inline script's. An
// inline script that page script inserted names its caller's place too, as
// setIntroductionInfoToCaller's "injectedScript" does: "<callerURL> line <callerLine> >
// injectedScript". Its errors and stacks name that place, and its top-level declarations are the
// global's, shared with the other scripts. What it throws, a SyntaxError included, is thrown.
NATIVE(Global_mrtClassicScript) {
  JS::CallArgs args = JS::CallArgsFromVp(argc, vp);
  std::string url, callerURL;
  JS::RootedString text(cx, JS::ToString(cx, args.get(0)));
  if (!text || !ArgUTF8(cx, args.get(1), url)) return false;
  uint32_t line = 1, column = 0, callerLine = 0;
  if (!JS::ToUint32(cx, args.get(2), &line) || !JS::ToUint32(cx, args.get(3), &column)) return false;
  bool introduced = args.get(4).isString();
  if (introduced && (!ArgUTF8(cx, args.get(4), callerURL) || !JS::ToUint32(cx, args.get(5), &callerLine))) return false;
  JS::CompileOptions options(cx);
  options.setFileAndLine(url.c_str(), line ? line : 1).setNoScriptRval(true).setIsRunOnce(true);
  if (column) options.setColumn(JS::ColumnNumberOneOrigin(column));
  if (introduced) options.setIntroductionInfo(callerURL.c_str(), "injectedScript", callerLine, 0);
  JS::AutoStableStringChars chars(cx);
  JS::SourceText<char16_t> source;
  if (!chars.initTwoByte(cx, text) ||
      !source.init(cx, chars.twoByteChars(), JS_GetStringLength(text), JS::SourceOwnership::Borrowed)) {
    return false;
  }
  JS::RootedValue rval(cx);
  if (!JS::Evaluate(cx, options, source, &rval)) return false;
  args.rval().setUndefined();
  return true;
}

// __mrtScriptPosition(element): a parser-made <script>'s [line, column] in its document, once (the
// script runs once); null for one script made.
NATIVE(Global_mrtScriptPosition) {
  JS::CallArgs args = JS::CallArgsFromVp(argc, vp);
  uint32_t id;
  if (!ArgNode(cx, args, 0, &id)) return false;
  uint64_t position = mrt_dom_take_script_position(tJs->host, id);
  if (!position) {
    args.rval().setNull();
    return true;
  }
  JS::RootedValueArray<2> values(cx);
  values[0].setNumber(uint32_t(position >> 32));
  values[1].setNumber(uint32_t(position));
  JSObject* array = JS::NewArrayObject(cx, values);
  if (!array) return false;
  args.rval().setObject(*array);
  return true;
}

// __mrtModuleStats(): [fetches, failed fetches, modules in the map] for the report.
NATIVE(Global_mrtModuleStats) {
  JS::CallArgs args = JS::CallArgsFromVp(argc, vp);
  ModuleLoader& loader = tJs->modules;
  size_t modules = 0;
  for (auto& [key, ms] : loader.mFetchedModules) modules += ms ? 1 : 0;
  JS::RootedValueArray<3> values(cx);
  values[0].setNumber(double(loader.mFetches));
  values[1].setNumber(double(loader.mFetchFailures));
  values[2].setNumber(double(modules + loader.mInline.size()));
  JSObject* array = JS::NewArrayObject(cx, values);
  if (!array) return false;
  args.rval().setObject(*array);
  return true;
}

static bool Illegal_constructor(JSContext* cx, unsigned argc, JS::Value* vp) {
  JS_ReportErrorUTF8(cx, "TypeError: Illegal constructor.");
  return false;
}

static bool SVGElement_constructor(JSContext* cx, unsigned argc, JS::Value* vp) {
  return ThrowTypeError(cx, "Illegal constructor.");
}

// A prototype in global slot `slot` whose own prototype is the one in `parent_slot` (or
// Object.prototype for -1), and a global constructor `name` for it, so that `instanceof` works.
// new EventTarget(): Gecko's ConstructibleEventTarget, an object with only EventTarget's members,
// whose prototype is new.target's (so classes can extend EventTarget). Its listeners are kept by
// the prelude's event code, as for any other target.
static const JSClass kEventTargetClass = {"EventTarget", 0};
static bool EventTarget_constructor(JSContext* cx, unsigned argc, JS::Value* vp) {
  JS::CallArgs args = JS::CallArgsFromVp(argc, vp);
  if (!args.isConstructing()) {
    JS_ReportErrorASCII(cx, "EventTarget constructor: 'new' is required");
    return false;
  }
  JSObject* obj = JS_NewObjectForConstructor(cx, &kEventTargetClass, args);
  if (!obj) return false;
  args.rval().setObject(*obj);
  return true;
}

static bool DefineInterface(JSContext* cx, JS::HandleObject global, const char* name, int slot, int parent_slot,
                            const JSPropertySpec* props, const JSFunctionSpec* fns,
                            const JSPropertySpec* props2 = nullptr, const JSFunctionSpec* fns2 = nullptr,
                            JSNative constructor = Illegal_constructor) {
  JS::RootedObject parent(cx);
  if (parent_slot >= 0) parent = &JS::GetReservedSlot(global, parent_slot).toObject();
  else parent = JS::GetRealmObjectPrototype(cx);
  JS::RootedObject proto(cx, JS_NewObjectWithGivenProto(cx, nullptr, parent));
  if (!proto) return false;
  if ((props && !JS_DefineProperties(cx, proto, props)) || (fns && !JS_DefineFunctions(cx, proto, fns)) ||
      (props2 && !JS_DefineProperties(cx, proto, props2)) || (fns2 && !JS_DefineFunctions(cx, proto, fns2))) {
    return false;
  }
  JSFunction* fun = JS_NewFunction(cx, constructor, 0, JSFUN_CONSTRUCTOR, name);
  if (!fun) return false;
  JS::RootedObject ctor(cx, JS_GetFunctionObject(fun));
  if (!JS_DefineProperty(cx, ctor, "prototype", proto, JSPROP_PERMANENT | JSPROP_READONLY) ||
      !JS_DefineProperty(cx, proto, "constructor", ctor, 0) || !JS_DefineProperty(cx, global, name, ctor, 0)) {
    return false;
  }
  JS_SetReservedSlot(global, slot, JS::ObjectValue(*proto));
  return true;
}

// The HTML element interfaces below HTMLElement are made on first use, as a Firefox window makes
// its interface objects (its resolve hook, WebIDLGlobalNameHash): when script names one on the
// global, or when an element of it gets its reflector. All 71 up front cost a page about 0.025 MB
// of realm and zone. Their members are written in bridge.js.
static bool EnsureHTMLInterface(JSContext* cx, JS::HandleObject global, int i) {
  if (JS::GetReservedSlot(global, SLOT_HTML_INTERFACES + i).isObject()) return true;
  int parent = kHTMLInterfaceParents[i];
  if (parent >= 0 && !EnsureHTMLInterface(cx, global, parent)) return false;
  // the native members of the few interfaces that have some
  const JSPropertySpec* props = !strcmp(kHTMLInterfaceNames[i], "HTMLTemplateElement") ? kHTMLTemplateElementProps : nullptr;
  const JSFunctionSpec* fns = !strcmp(kHTMLInterfaceNames[i], "HTMLSlotElement") ? kHTMLSlotElementFns : nullptr;
  return DefineInterface(cx, global, kHTMLInterfaceNames[i], SLOT_HTML_INTERFACES + i,
                         parent < 0 ? SLOT_HTML_ELEMENT : SLOT_HTML_INTERFACES + parent, props, fns);
}

static std::vector<MrtJs::FrameWindow*> ChildWindows(uint32_t aDoc);
static bool SetGlobal(JSContext* cx, JSObject* aGlobal, JS::MutableHandleValue rval);

// window[i]: the i-th frame of the window, as WindowProxy's indexed getter (nsGlobalWindowOuter::
// IndexedGetterOuter) gives it; the getter is made by the resolve hook when a frame first is at i.
static bool FrameIndexGetter(JSContext* cx, unsigned argc, JS::Value* vp) {
  JS::CallArgs args = JS::CallArgsFromVp(argc, vp);
  size_t i = size_t(js::GetFunctionNativeReserved(&args.callee(), 0).toInt32());
  JSObject* win = args.thisv().isObject() ? &args.thisv().toObject() : JS::CurrentGlobalOrNull(cx);
  MrtJs::FrameWindow* w = FrameWindowOf(win);
  std::vector<MrtJs::FrameWindow*> frames = ChildWindows(w ? w->doc : 1);
  if (i >= frames.size()) {
    args.rval().setUndefined();
    return true;
  }
  return ViewOfWindow(cx, *frames[i]->global, args.rval());
}

static bool Global_resolve(JSContext* cx, JS::HandleObject obj, JS::HandleId id, bool* resolvedp) {
  if (id.isInt() && id.toInt() >= 0 && tJs && !tJs->windows.empty()) {
    MrtJs::FrameWindow* w = FrameWindowOf(obj);
    if (size_t(id.toInt()) < ChildWindows(w ? w->doc : 1).size()) {
      JSFunction* fn = js::NewFunctionWithReserved(cx, FrameIndexGetter, 0, 0, "get");
      if (!fn) return false;
      js::SetFunctionNativeReserved(JS_GetFunctionObject(fn), 0, JS::Int32Value(id.toInt()));
      JS::RootedObject getter(cx, JS_GetFunctionObject(fn));
      if (!JS_DefinePropertyById(cx, obj, id, getter, nullptr, JSPROP_ENUMERATE)) return false;
      *resolvedp = true;
      return true;
    }
  }
  if (!JS_ResolveStandardClass(cx, obj, id, resolvedp)) return false;
  // only once InitGlobal has made HTMLElement, their parent
  if (*resolvedp || !id.isString() || !JS::GetReservedSlot(obj, SLOT_HTML_ELEMENT).isObject()) return true;
  JSLinearString* name = id.toLinearString();
  for (int i = 0; i < HTML_INTERFACE_COUNT; i++) {
    if (JS_LinearStringEqualsAscii(name, kHTMLInterfaceNames[i])) {
      if (!EnsureHTMLInterface(cx, obj, i)) return false;
      *resolvedp = true;
      return true;
    }
  }
  return true;
}

// Whether the resolve hook may define `id`: the standard classes, and any name starting "HTML".
static bool Global_mayResolve(const JSAtomState& names, jsid id, JSObject* maybeObj) {
  // window[i], a frame (Global_resolve)
  if (id.isInt()) return true;
  if (JS_MayResolveStandardClass(names, id, maybeObj)) return true;
  if (!id.isString()) return false;
  JSLinearString* s = id.toLinearString();
  return JS::GetLinearStringLength(s) > 4 && JS::GetLinearStringCharAt(s, 0) == 'H' &&
         JS::GetLinearStringCharAt(s, 1) == 'T' && JS::GetLinearStringCharAt(s, 2) == 'M' &&
         JS::GetLinearStringCharAt(s, 3) == 'L';
}

// The argument, a string, as UTF-16 (nsString).
static bool ArgUTF16(JSContext* cx, JS::HandleValue v, std::u16string& out) {
  JS::RootedString str(cx, JS::ToString(cx, v));
  if (!str) return false;
  out.resize(JS_GetStringLength(str));
  return JS_CopyStringChars(cx, mozilla::Range<char16_t>(out.data(), out.size()), str);
}

static bool NewUTF16(JSContext* cx, const std::u16string& s, JS::MutableHandleValue rval) {
  JSString* str = JS_NewUCStringCopyN(cx, s.data(), s.size());
  if (!str) return false;
  rval.setString(str);
  return true;
}

// __mrtSessionStorage(origin, op, key, value): SessionStorage's methods over the origin's cache.
// op: "length", "key" (key is the index), "getItem", "setItem" (false if the quota is exceeded:
// the caller throws QuotaExceededError), "removeItem", "clear", "names" (GetSupportedNames).
static bool Global_mrtSessionStorage(JSContext* cx, unsigned argc, JS::Value* vp) {
  JS::CallArgs args = JS::CallArgsFromVp(argc, vp);
  std::string origin, op;
  if (!ArgUTF8(cx, args.get(0), origin) || !ArgUTF8(cx, args.get(1), op)) return false;
  SessionStorageCache& cache = tJs->sessionStorage[origin];
  std::u16string key, value;
  if (op == "length") {
    args.rval().setNumber(cache.Length());
  } else if (op == "key") {
    // the index is an unsigned long, converted by the caller (ToUint32)
    double index;
    if (!JS::ToNumber(cx, args.get(2), &index)) return false;
    if (!(index >= 0) || index >= cache.Length() || !cache.Key(uint32_t(index), key)) {
      args.rval().setNull();
      return true;
    }
    return NewUTF16(cx, key, args.rval());
  } else if (op == "getItem") {
    if (!ArgUTF16(cx, args.get(2), key)) return false;
    if (!cache.GetItem(key, value)) {
      args.rval().setNull();
      return true;
    }
    return NewUTF16(cx, value, args.rval());
  } else if (op == "setItem") {
    if (!ArgUTF16(cx, args.get(2), key) || !ArgUTF16(cx, args.get(3), value)) return false;
    args.rval().setBoolean(cache.SetItem(key, value) != SessionStorageCache::kQuotaExceeded);
  } else if (op == "removeItem") {
    if (!ArgUTF16(cx, args.get(2), key)) return false;
    cache.RemoveItem(key);
    args.rval().setUndefined();
  } else if (op == "clear") {
    cache.Clear();
    args.rval().setUndefined();
  } else if (op == "names") {
    JS::RootedObject names(cx, JS::NewArrayObject(cx, cache.Length()));
    if (!names) return false;
    JS::RootedValue name(cx);
    for (uint32_t i = 0; i < cache.Length(); i++) {
      if (!cache.Key(i, key) || !NewUTF16(cx, key, &name) || !JS_SetElement(cx, names, i, name)) return false;
    }
    args.rval().setObject(*names);
  } else {
    JS_ReportErrorASCII(cx, "__mrtSessionStorage: unknown op");
    return false;
  }
  return true;
}

size_t mrt_js_session_storage_bytes(MrtJs* js) {
  size_t n = 0;
  for (auto& [origin, cache] : js->sessionStorage) n += origin.capacity() + cache.SizeOfExcludingThis();
  return n;
}

static bool Global_mrtWindows(JSContext* cx, unsigned argc, JS::Value* vp);
static bool Global_mrtPostMessage(JSContext* cx, unsigned argc, JS::Value* vp);
static bool Global_mrtWithIncumbent(JSContext* cx, unsigned argc, JS::Value* vp);
static bool Global_mrtEmulateUndefined(JSContext* cx, unsigned argc, JS::Value* vp);

// `doc`: the window's document (1, the page's, for the page's window). A frame's `document` can be
// replaced when it navigates away from its initial about:blank (the window is reused, as Gecko
// reuses the inner window then).
static bool Global_mrtWorker(JSContext* cx, unsigned argc, JS::Value* vp);
static bool Global_mrtBlobURL(JSContext* cx, unsigned argc, JS::Value* vp);
static bool Global_mrtCompression(JSContext* cx, unsigned argc, JS::Value* vp);
static bool Global_mrtDebuggerEval(JSContext* cx, unsigned argc, JS::Value* vp);

static bool InitGlobal(JSContext* cx, JS::HandleObject global, uint32_t doc = 1) {
  // No JS::InitRealmStandardClasses: the built-in classes are made on first use by the resolve hook
  // of JS::DefaultGlobalClassOps, as on a Firefox window. Making all of them up front left about
  // 0.1 MB more in the page's realm and zone.
  if (!DefineInterface(cx, global, "EventTarget", SLOT_EVENT_TARGET, -1, nullptr, nullptr, nullptr, nullptr,
                       EventTarget_constructor) ||
      !DefineInterface(cx, global, "Window", SLOT_WINDOW, SLOT_EVENT_TARGET, nullptr, nullptr) ||
      !DefineInterface(cx, global, "Node", SLOT_NODE, SLOT_EVENT_TARGET, kNodeProps, kNodeFns) ||
      !DefineInterface(cx, global, "Element", SLOT_ELEMENT, SLOT_NODE, kElementProps, kElementFns, kParentNodeProps,
                       kParentNodeFns) ||
      !DefineInterface(cx, global, "HTMLElement", SLOT_HTML_ELEMENT, SLOT_ELEMENT, kHTMLElementProps, nullptr, nullptr,
                       nullptr, HTMLConstructor) ||
      !DefineInterface(cx, global, "SVGElement", SLOT_SVG_ELEMENT, SLOT_ELEMENT, nullptr, nullptr, nullptr, nullptr,
                       SVGElement_constructor) ||
      !DefineInterface(cx, global, "CharacterData", SLOT_CHARACTER_DATA, SLOT_NODE, kCharacterDataProps, nullptr) ||
      !DefineInterface(cx, global, "Text", SLOT_TEXT, SLOT_CHARACTER_DATA, kSlottableProps, nullptr) ||
      !DefineInterface(cx, global, "Comment", SLOT_COMMENT, SLOT_CHARACTER_DATA, nullptr, nullptr) ||
      !DefineInterface(cx, global, "CDATASection", SLOT_CDATA_SECTION, SLOT_TEXT, nullptr, nullptr) ||
      !DefineInterface(cx, global, "ProcessingInstruction", SLOT_PROCESSING_INSTRUCTION, SLOT_CHARACTER_DATA, nullptr, nullptr) ||
      !DefineInterface(cx, global, "Document", SLOT_DOCUMENT, SLOT_NODE, kDocumentProps, kDocumentFns,
                       kParentNodeProps, kParentNodeFns) ||
      !DefineInterface(cx, global, "DocumentType", SLOT_DOCUMENT_TYPE, SLOT_NODE, kDocumentTypeProps, nullptr) ||
      !DefineInterface(cx, global, "DocumentFragment", SLOT_DOCUMENT_FRAGMENT, SLOT_NODE, kParentNodeProps,
                       kParentNodeFns) ||
      !DefineInterface(cx, global, "ShadowRoot", SLOT_SHADOW_ROOT, SLOT_DOCUMENT_FRAGMENT, kShadowRootProps,
                       kShadowRootFns) ||
      !DefineInterface(cx, global, "MutationObserver", SLOT_MUTATION_OBSERVER, -1, nullptr, kMutationObserverFns,
                       nullptr, nullptr, MutationObserver_constructor) ||
      !DefineInterface(cx, global, "MutationRecord", SLOT_MUTATION_RECORD, -1, kMutationRecordProps, nullptr) ||
      !DefineInterface(cx, global, "CustomElementRegistry", SLOT_CUSTOM_ELEMENT_REGISTRY, -1, nullptr,
                       kCustomElementRegistryFns)) {
    return false;
  }
  // Symbol.toStringTag of the interfaces whose objects script inspects that way (the WebIDL
  // bindings give every prototype one)
  for (auto [slot, name] : {std::pair{SLOT_EVENT_TARGET, "EventTarget"}, std::pair{SLOT_MUTATION_OBSERVER, "MutationObserver"},
                            std::pair{SLOT_MUTATION_RECORD, "MutationRecord"}}) {
    JS::RootedObject proto(cx, &JS::GetReservedSlot(global, slot).toObject());
    JS::RootedId tag(cx, JS::PropertyKey::Symbol(JS::GetWellKnownSymbol(cx, JS::SymbolCode::toStringTag)));
    JS::RootedValue value(cx);
    if (!NewString(cx, name, strlen(name), &value) || !JS_DefinePropertyById(cx, proto, tag, value, JSPROP_READONLY)) return false;
  }
  // the HTML element interfaces are made on first use (EnsureHTMLInterface)
  if (!JS_DefineFunction(cx, global, "__mrtRejectionTask", Global_mrtRejectionTask, 1, 0) ||
      !JS_DefineFunction(cx, global, "__mrtEnterCallback", Global_mrtEnterCallback, 0, 0) ||
      !JS_DefineFunction(cx, global, "__mrtLeaveCallback", Global_mrtLeaveCallback, 0, 0) ||
      !JS_DefineFunction(cx, global, "__mrtStyle", Global_mrtStyle, 5, 0) ||
      !JS_DefineFunction(cx, global, "__mrtComputed", Global_mrtComputed, 4, 0) ||
      !JS_DefineFunction(cx, global, "__mrtBridge", Global_mrtBridge, 1, 0) ||
      !JS_DefineFunction(cx, global, "__mrtParseFragment", Global_mrtParseFragment, 2, 0) ||
      !JS_DefineFunction(cx, global, "__mrtCEReactions", Global_mrtCEReactions, 2, 0) ||
      !JS_DefineFunction(cx, global, "__mrtLink", Global_mrtLink, 2, 0) ||
      !JS_DefineFunction(cx, global, "__mrtCreateHTMLDocument", Global_mrtCreateHTMLDocument, 1, 0) ||
      !JS_DefineFunction(cx, global, "__mrtParseDocument", Global_mrtParseDocument, 1, 0) ||
      !JS_DefineFunction(cx, global, "__mrtSheet", Global_mrtSheet, 3, 0) ||
      !JS_DefineFunction(cx, global, "__mrtBorderBox", Global_mrtBorderBox, 1, 0) ||
      !JS_DefineFunction(cx, global, "__mrtClientScroll", Global_mrtClientScroll, 1, 0) ||
      !JS_DefineFunction(cx, global, "__mrtViewport", Global_mrtViewport, 1, 0) ||
      !JS_DefineFunction(cx, global, "__mrtRootScroll", Global_mrtRootScroll, 2, 0) ||
      !JS_DefineFunction(cx, global, "__mrtElementScroll", Global_mrtElementScroll, 3, 0) ||
      !JS_DefineFunction(cx, global, "__mrtRefresh", Global_mrtRefresh, 1, 0) ||
      !JS_DefineFunction(cx, global, "__mrtEachWindow", Global_mrtEachWindow, 1, 0) ||
      !JS_DefineFunction(cx, global, "__mrtResizeBoxes", Global_mrtResizeBoxes, 2, 0) ||
      !JS_DefineFunction(cx, global, "__mrtIntersections", Global_mrtIntersections, 4, 0) ||
      !JS_DefineFunction(cx, global, "__mrtNodesFromPoint", Global_mrtNodesFromPoint, 4, 0) ||
      !JS_DefineFunction(cx, global, "__mrtInnerText", Global_mrtInnerText, 1, 0) ||
      !JS_DefineFunction(cx, global, "__mrtLinkSheet", Global_mrtLinkSheet, 3, 0) ||
      !JS_DefineFunction(cx, global, "__mrtCssom", Global_mrtCssom, 4, 0) ||
      !JS_DefineFunction(cx, global, "__mrtDebuggerEval", Global_mrtDebuggerEval, 1, 0) ||
      !JS_DefineFunction(cx, global, "__mrtModuleScript", Global_mrtModuleScript, 2, 0) ||
      !JS_DefineFunction(cx, global, "__mrtClassicScript", Global_mrtClassicScript, 6, 0) ||
      !JS_DefineFunction(cx, global, "__mrtScriptPosition", Global_mrtScriptPosition, 1, 0) ||
      !JS_DefineFunction(cx, global, "__mrtModuleStats", Global_mrtModuleStats, 0, 0) ||
      !JS_DefineFunction(cx, global, "__mrtSetInnerHTML", Global_mrtSetInnerHTML, 2, 0) ||
      !JS_DefineFunction(cx, global, "__mrtReplaceChildren", Global_mrtReplaceChildren, 2, 0) ||
      !JS_DefineFunction(cx, global, "__mrtSessionStorage", Global_mrtSessionStorage, 4, 0) ||
      !JS_DefineFunction(cx, global, "__mrtWindows", Global_mrtWindows, 3, 0) ||
      !JS_DefineFunction(cx, global, "__mrtPostMessage", Global_mrtPostMessage, 1, 0) ||
      !JS_DefineFunction(cx, global, "__mrtWithIncumbent", Global_mrtWithIncumbent, 3, 0) ||
      !JS_DefineFunction(cx, global, "__mrtEmulateUndefined", Global_mrtEmulateUndefined, 1, 0) ||
      !JS_DefineFunction(cx, global, "__mrtWorker", Global_mrtWorker, 6, 0) ||
      !JS_DefineFunction(cx, global, "__mrtBlobURL", Global_mrtBlobURL, 6, 0) ||
      !JS_DefineFunction(cx, global, "__mrtCompression", Global_mrtCompression, 4, 0)) {
    return false;
  }
  // the window is a Window: its prototype chain is Window.prototype, EventTarget.prototype (Gecko has
  // a WindowProperties object between them for named properties, not made here)
  JS::RootedObject windowProto(cx, &JS::GetReservedSlot(global, SLOT_WINDOW).toObject());
  if (!JS_SetPrototype(cx, global, windowProto)) return false;
  // window.customElements: the document's one registry
  JS::RootedObject registryProto(cx, &JS::GetReservedSlot(global, SLOT_CUSTOM_ELEMENT_REGISTRY).toObject());
  JS::RootedObject registry(cx, JS_NewObjectWithGivenProto(cx, nullptr, registryProto));
  if (!registry || !JS_DefineProperty(cx, global, "customElements", registry, JSPROP_ENUMERATE)) return false;
  JS::RootedObject console(cx, JS_NewPlainObject(cx));
  if (!console || !JS_DefineFunctions(cx, console, kConsoleFns) ||
      !JS_DefineProperty(cx, global, "console", console, 0)) {
    return false;
  }
  JS::RootedValue docValue(cx);
  if (!Wrap(cx, doc, &docValue)) return false;
  JS::RootedValue self(cx, JS::ObjectValue(*global));
  unsigned docAttrs = doc == 1 ? JSPROP_ENUMERATE | JSPROP_READONLY | JSPROP_PERMANENT : JSPROP_ENUMERATE | JSPROP_READONLY;
  return JS_DefineProperty(cx, global, "document", docValue, docAttrs) &&
         JS_DefineProperty(cx, global, "window", self, JSPROP_ENUMERATE | JSPROP_READONLY | JSPROP_PERMANENT) &&
         JS_DefineProperty(cx, global, "self", self, JSPROP_ENUMERATE);
}

// ---- the C interface ----

// ---- The page's global, as Gecko makes an inner window's (nsGlobalWindowOuter.cpp) ----
//
// A global's principal is its document's origin. Gecko also keeps it on the compartment
// (xpc::CompartmentPrivate), which is how SelectZone finds a same-origin compartment to reuse.
// Content principals only: the mini runtime has no system or add-on code.

struct MrtPrincipals final : JSPrincipals {
  std::string origin;
  explicit MrtPrincipals(std::string aOrigin) : origin(std::move(aOrigin)) {}
  // Principals are written only by structured clone of a SavedFrame, which the bridge never does.
  bool write(JSContext*, JSStructuredCloneWriter*) override { return false; }
  bool isSystemPrincipal() override { return false; }
  bool isAddonPrincipal() override { return false; }
  // An opaque origin ("null") is unique: it is the same as itself only.
  bool Equals(const MrtPrincipals* aOther) const {
    return this == aOther || (origin != "null" && origin == aOther->origin);
  }
};

// Whether two windows' globals are of the same origin (their realms' principals).
static bool SameOriginGlobals(JSObject* aFirst, JSObject* aSecond) {
  if (aFirst == aSecond) return true;
  auto* a = static_cast<MrtPrincipals*>(JS::GetRealmPrincipals(JS::GetObjectRealmOrNull(aFirst)));
  auto* b = static_cast<MrtPrincipals*>(JS::GetRealmPrincipals(JS::GetObjectRealmOrNull(aSecond)));
  return a && b && a->Equals(b);
}

// Window `aTarget` as the current window sees it: itself when they are of the same origin, else the
// current window's cross-origin view of it (bridge.js's __mrt.crossOriginView, Gecko's cross-origin
// WindowProxy). Windows of one site share this runtime (SPEC decision 19) but not an origin; the
// engine's wrappers between their compartments let everything through, so the view is ours.
static bool ViewOfWindow(JSContext* cx, JSObject* aTarget, JS::MutableHandleValue rval) {
  if (!aTarget) {
    rval.setNull();
    return true;
  }
  JS::RootedObject current(cx, JS::CurrentGlobalOrNull(cx));
  rval.setObject(*aTarget);
  if (!JS_WrapValue(cx, rval)) return false;
  if (!current || SameOriginGlobals(current, aTarget)) return true;
  JS::RootedValue mrt(cx), fn(cx);
  if (!JS_GetProperty(cx, current, "__mrt", &mrt) || !mrt.isObject()) return true;
  JS::RootedObject mrtObj(cx, &mrt.toObject());
  if (!JS_GetProperty(cx, mrtObj, "crossOriginView", &fn) || !fn.isObject()) return true;
  JS::RootedValue target(cx, rval);
  return JS_CallFunctionValue(cx, mrtObj, fn, JS::HandleValueArray(target), rval);
}

static void DestroyPrincipals(JSPrincipals* aPrincipals) { delete static_cast<MrtPrincipals*>(aPrincipals); }

// The JS engine's same-origin check (Gecko's nsJSPrincipals::Subsumes for content principals:
// equal origins; document.domain is not supported).
static bool Subsumes(JSPrincipals* aFirst, JSPrincipals* aSecond) {
  if (!aFirst || !aSecond) return aFirst == aSecond;
  return static_cast<MrtPrincipals*>(aFirst)->Equals(static_cast<MrtPrincipals*>(aSecond));
}

// The current global's __mrt.trustedTypes (prelude.js), or false (the prelude has not run yet).
static bool TrustedTypesOf(JSContext* cx, JS::MutableHandleObject aOut) {
  JS::RootedObject global(cx, JS::CurrentGlobalOrNull(cx));
  JS::RootedValue mrt(cx), tt(cx);
  if (!global || !JS_GetProperty(cx, global, "__mrt", &mrt) || !mrt.isObject()) return false;
  JS::RootedObject mrtObj(cx, &mrt.toObject());
  if (!JS_GetProperty(cx, mrtObj, "trustedTypes", &tt) || !tt.isObject()) return false;
  aOut.set(&tt.toObject());
  return true;
}

// Trusted Types' eval hooks, as Gecko's (nsJSEnvironment's and RuntimeService's JSSecurityCallbacks).
// TrustedTypeUtils::HostGetCodeForEval: eval of a TrustedScript compiles its string.
static bool CodeForEvalGets(JSContext* cx, JS::HandleObject aCode, JS::MutableHandle<JSString*> aOutCode) {
  aOutCode.set(nullptr);
  JS::RootedObject tt(cx);
  if (!TrustedTypesOf(cx, &tt)) {
    JS_ClearPendingException(cx);
    return true;
  }
  JS::RootedValue arg(cx, JS::ObjectValue(*aCode)), rval(cx);
  if (!JS_CallFunctionName(cx, tt, "scriptData", JS::HandleValueArray(arg), &rval)) return false;
  if (rval.isString()) aOutCode.set(rval.toString());
  return true;
}
// AreArgumentsTrustedForEnsureCSPDoesNotBlockStringCompilation: whether eval or the Function
// constructor may compile a string, under the document's require-trusted-types-for (prelude.js's
// evalAllowed). The engine throws the EvalError when it may not.
static bool ContentSecurityPolicyAllows(JSContext* cx, JS::RuntimeCode aKind, JS::Handle<JSString*> aCodeString,
                                        JS::CompilationType aCompilationType,
                                        JS::Handle<JS::StackGCVector<JSString*>> aParameterStrings,
                                        JS::Handle<JSString*> aBodyString,
                                        JS::Handle<JS::StackGCVector<JS::Value>> aParameterArgs,
                                        JS::Handle<JS::Value> aBodyArg, bool* aOutCanCompileStrings) {
  *aOutCanCompileStrings = true;
  if (aKind != JS::RuntimeCode::JS || aCompilationType == JS::CompilationType::Undefined) return true;
  JS::RootedObject tt(cx);
  if (!TrustedTypesOf(cx, &tt)) {
    JS_ClearPendingException(cx);
    return true;
  }
  JS::RootedValueArray<6> args(cx);
  args[0].setString(aCodeString);
  args[1].setBoolean(aCompilationType == JS::CompilationType::Function);
  if (aBodyString) args[2].setString(aBodyString);
  else args[2].setNull();
  args[3].set(aBodyArg);
  JS::RootedValueVector strings(cx), values(cx);
  for (size_t i = 0; i < aParameterStrings.length(); i++) {
    if (!strings.append(JS::StringValue(aParameterStrings[i]))) return false;
  }
  for (size_t i = 0; i < aParameterArgs.length(); i++) {
    if (!values.append(aParameterArgs[i])) return false;
  }
  JSObject* stringArray = JS::NewArrayObject(cx, strings);
  if (!stringArray) return false;
  args[4].setObject(*stringArray);
  JSObject* valueArray = JS::NewArrayObject(cx, values);
  if (!valueArray) return false;
  args[5].setObject(*valueArray);
  JS::RootedValue rval(cx);
  if (!JS_CallFunctionName(cx, tt, "evalAllowed", args, &rval)) return false;
  *aOutCanCompileStrings = JS::ToBoolean(rval);
  return true;
}

static const JSSecurityCallbacks kSecurityCallbacks = {ContentSecurityPolicyAllows, CodeForEvalGets, Subsumes};

static void DestroyCompartment(JS::GCContext*, JS::Compartment* aCompartment) {
  if (auto* principal = static_cast<MrtPrincipals*>(JS_GetCompartmentPrivate(aCompartment))) {
    JS_SetCompartmentPrivate(aCompartment, nullptr);
    if (--principal->refcount == 0) DestroyPrincipals(principal);
  }
}

struct CompartmentFinderState {
  explicit CompartmentFinderState(MrtPrincipals* aPrincipal) : principal(aPrincipal), compartment(nullptr) {}

  // Input: we look for a compartment which is same-origin with the given principal.
  MrtPrincipals* principal;

  // Output: We set this member if we find a compartment.
  JS::Compartment* compartment;
};

static JS::CompartmentIterResult FindSameOriginCompartment(JSContext* aCx, void* aData,
                                                           JS::Compartment* aCompartment) {
  auto* data = static_cast<CompartmentFinderState*>(aData);

  // If this compartment is not safe to share across globals, don't do anything with it; in
  // particular we should not be getting its private data, because it may be in the middle of
  // being collected.
  if (!js::IsSharableCompartment(aCompartment)) {
    return JS::CompartmentIterResult::KeepGoing;
  }

  auto* compartmentPrincipal = static_cast<MrtPrincipals*>(JS_GetCompartmentPrivate(aCompartment));
  if (!compartmentPrincipal || !compartmentPrincipal->Equals(data->principal)) {
    // Can't reuse this one, keep going.
    return JS::CompartmentIterResult::KeepGoing;
  }

  // We have a winner!
  data->compartment = aCompartment;
  return JS::CompartmentIterResult::Stop;
}

// Gecko's SelectZone. `aAncestor` is the in-process ancestor window highest in the hierarchy (the
// top-level page's global for a frame), null for a top-level load.
static JS::RealmCreationOptions& SelectZone(JSContext* aCx, MrtPrincipals* aPrincipal, JS::HandleObject aAncestor,
                                            JS::RealmCreationOptions& aOptions) {
  if (!aAncestor) {
    // We're a toplevel load.  Use a new zone.  This way, when we do zone-based compartment
    // sharing we won't share compartments across navigations.
    return aOptions.setNewCompartmentAndZone();
  }

  // If we have an ancestor window, use its zone.
  JS::Zone* zone = JS::GetObjectZone(aAncestor);
  // Now try to find an existing compartment that's same-origin with our principal.
  CompartmentFinderState data(aPrincipal);
  JS_IterateCompartmentsInZone(aCx, zone, &data, FindSameOriginCompartment);
  if (data.compartment) {
    return aOptions.setExistingCompartment(data.compartment);
  }
  return aOptions.setNewCompartmentInExistingZone(aAncestor);
}

// A page's global with the options Gecko's CreateNativeGlobalForInner gives a content window:
// the zone from SelectZone, the SharedArrayBuffer constructor only when the page is cross-origin
// isolated (bug 1624266), the prefable options, and xpc::InitGlobalObjectOptions for a content principal (the secure
// context; no resist-fingerprinting time zone, locale or fdlibm overrides, as in a default
// profile). The principal is put on a new compartment, as xpc::CompartmentPrivate holds it.
static JSObject* CreateNativeGlobal(JSContext* aCx, const MrtGlobalOptions* aGlobal, JS::HandleObject aAncestor,
                                    const JSClass* aClass = &kGlobalClass) {
  auto* principal = new MrtPrincipals(std::string(aGlobal->origin, aGlobal->origin_len));
  JS::RealmOptions options;
  JS::RealmCreationOptions& creationOptions = options.creationOptions();
  SelectZone(aCx, principal, aAncestor, creationOptions);
  creationOptions.setDefineSharedArrayBufferConstructor(aGlobal->cross_origin_isolated != 0);
  // xpc::SetPrefableRealmOptions, which every window global gets when it is wrapped
  // (XPCWrappedNative.cpp), with the prefs at their defaults: javascript.options.shared_memory,
  // browser.tabs.remote.useCrossOriginOpenerPolicy and useCrossOriginEmbedderPolicy all true
  creationOptions.setSharedMemoryAndAtomicsEnabled(true).setCoopAndCoepEnabled(true);
  creationOptions.setSecureContext(aGlobal->secure_context != 0);
  JS_HoldPrincipals(principal);
  JS::RootedObject global(aCx, JS_NewGlobalObject(aCx, aClass, principal, JS::FireOnNewGlobalHook, options));
  if (global) {
    JS::Compartment* compartment = JS::GetCompartment(global);
    if (!JS_GetCompartmentPrivate(compartment)) {
      JS_HoldPrincipals(principal);
      JS_SetCompartmentPrivate(compartment, principal);
    }
  }
  JS_DropPrincipals(aCx, principal);
  return global;
}

// isSecureContext and crossOriginIsolated, as the window answers them, from the realm's options.
static bool DefineWindowSecurityProperties(JSContext* aCx, JS::HandleObject aGlobal, bool aCrossOriginIsolated) {
  JS::Realm* realm = JS::GetObjectRealmOrNull(aGlobal);
  JS::RootedValue secure(aCx, JS::BooleanValue(JS::GetIsSecureContext(realm)));
  JS::RootedValue isolated(aCx, JS::BooleanValue(aCrossOriginIsolated));
  return JS_DefineProperty(aCx, aGlobal, "isSecureContext", secure, JSPROP_ENUMERATE) &&
         JS_DefineProperty(aCx, aGlobal, "crossOriginIsolated", isolated, JSPROP_ENUMERATE);
}

MrtJs* mrt_js_new(void* host, const MrtGlobalOptions* page) {
  // once per process, from whichever thread comes first (a function-local static is thread-safe)
  static const bool sInitialized = JS_Init();
  if (!sInitialized) return nullptr;
  auto js = std::make_unique<MrtJs>();
  js->host = host;
  js->cx = JS_NewContext(JS::DefaultHeapMaxBytes);
  // the shell's job queue, so that promise reactions run (after each script, as a microtask
  // checkpoint does after a script element)
  if (!js->cx || !js::UseInternalJobQueues(js->cx) || !JS::InitSelfHostedCode(js->cx)) return nullptr;
  tJs = js.get();
  JSContext* cx = js->cx;
  // XPCJSRuntime::Initialize: no cap on the GC heap (0xffffffff is infinity for uint32_t
  // parameters). With JS::DefaultHeapMaxBytes (32 MB) YouTube's watch page ran out of memory at a
  // 38 MB heap, where Firefox's window holds 84 MB.
  JS_SetGCParameter(cx, JSGC_MAX_BYTES, 0xffffffff);
  JS::SetPromiseRejectionTrackerCallback(cx, RejectionTracker, nullptr);
  JS_InitDestroyPrincipalsCallback(cx, DestroyPrincipals);
  JS_SetSecurityCallbacks(cx, &kSecurityCallbacks);
  JS_SetDestroyCompartmentCallback(cx, DestroyCompartment);
  // ModuleLoaderBase::EnsureModuleHooksInitialized, and the preparer the engine reports through
  JS::SetModuleLoadHook(JS_GetRuntime(cx), HostLoadImportedModule);
  JS::SetModuleMetadataHook(JS_GetRuntime(cx), HostPopulateImportMeta);
  js::SetScriptEnvironmentPreparer(cx, &sEnvironmentPreparer);
  JS::RootedObject global(cx, CreateNativeGlobal(cx, page, nullptr));
  if (!global) return nullptr;
  js->global = std::make_unique<JS::PersistentRootedObject>(cx, global);
  JS_AddExtraGCRootsTracer(cx, TracePreserved, js.get());
  JS_AddWeakPointerZonesCallback(cx, SweepCache, js.get());
  JS::EnterRealm(cx, global);
  if (!InitGlobal(cx, global) || !DefineWindowSecurityProperties(cx, global, page->cross_origin_isolated != 0)) {
    fprintf(stderr, "mrt-js: setting up the global failed\n");
    return nullptr;
  }
  return js.release();
}

void mrt_js_free(MrtJs* js) {
  JSContext* cx = js->cx;
  JS::LeaveRealm(cx, nullptr);
  JS_RemoveExtraGCRootsTracer(cx, TracePreserved, js);
  JS_RemoveWeakPointerZonesCallback(cx, SweepCache);
  js->frames.clear();
  js->uncaught.clear();
  js->aboutToBeNotified.clear();
  js->rejectionHandled.clear();
  js->notifyBatches.clear();
  js->handledBatches.clear();
  js->global.reset();
  js->cache.clear();
  js->ce = CustomElements();  // its JS::Heap pointers, while the context is alive
  js->modules = ModuleLoader();
  // the reflectors forget their observers, which the last GC would otherwise trace after they are freed
  for (auto& o : js->mo.mObservers) {
    if (JSObject* reflector = o->mReflector.unbarrieredGet()) JS::SetReservedSlot(reflector, 0, JS::UndefinedValue());
  }
  js->mo = MutationObservers();
  JS_DestroyContext(cx);
  if (tJs == js) tJs = nullptr;
  delete js;
}

int32_t mrt_js_new_frame_global(MrtJs* js, const MrtGlobalOptions* frame) {
  JSContext* cx = js->cx;
  JS::RootedObject global(cx, CreateNativeGlobal(cx, frame, *js->global));
  if (!global) return -1;
  js->frames.push_back(std::make_unique<JS::PersistentRootedObject>(cx, global));
  return int32_t(js->frames.size() - 1);
}

int32_t mrt_js_frame_global_relation(MrtJs* js, int32_t i) {
  if (i < 0 || size_t(i) >= js->frames.size()) return -1;
  JSObject* frame = *js->frames[i];
  JSObject* page = *js->global;
  int32_t r = 0;
  if (JS::GetObjectZone(frame) == JS::GetObjectZone(page)) r |= 1;
  if (JS::GetCompartment(frame) == JS::GetCompartment(page)) r |= 2;
  if (JS::GetIsSecureContext(JS::GetObjectRealmOrNull(frame))) r |= 4;
  for (size_t k = 0; k < size_t(i); k++)
    if (JS::GetCompartment(*js->frames[k]) == JS::GetCompartment(frame)) r |= 8;
  return r;
}

// ---- Dedicated workers (DESIGN, "Workers: the build plan", W1) ----
//
// A worker runs on a thread of its own with a JS context of its own (Gecko's WorkerThread and
// WorkerJSContext), its global a DedicatedWorkerGlobalScope: EventTarget, then WorkerGlobalScope, then
// DedicatedWorkerGlobalScope, with no node interfaces and no document. The Rust side (worker.rs) runs
// the thread; prelude.js and bridge.js run in the global with what is [Exposed=Window] only left out
// (they test __mrtWorkerScope), and bridge.js's worker section puts each member where Firefox has it.

static const JSClass kWorkerGlobalClass = {"DedicatedWorkerGlobalScope",
                                           JSCLASS_GLOBAL_FLAGS_WITH_SLOTS(SLOT_COUNT - JSCLASS_GLOBAL_SLOT_COUNT),
                                           &JS::DefaultGlobalClassOps};
// a shared worker's (GAPS B85): SharedWorkerGlobalScope in place of DedicatedWorkerGlobalScope
static const JSClass kSharedWorkerGlobalClass = {"SharedWorkerGlobalScope",
                                                 JSCLASS_GLOBAL_FLAGS_WITH_SLOTS(SLOT_COUNT - JSCLASS_GLOBAL_SLOT_COUNT),
                                                 &JS::DefaultGlobalClassOps};

// The process's blob: URLs (Gecko's BlobURLProtocolHandler gDataTable): one table for the page's and
// the workers' threads, behind a mutex, as Gecko's sMutex. An entry keeps a copy of the Blob's bytes
// (Gecko keeps a reference to its BlobImpl; here a Blob's bytes live in one thread's JS heap), its
// type, the origin that made it and, for a worker's URL, the worker's tag, so the worker's end revokes
// it (WorkerGlobalScope::UnregisterHostObjectURI). A revoked entry is found only by a load that
// parsed the URL before the revoke (GetDataEntry's aAlsoIfRevoked), for RELEASING_TIMER (5 s) on the
// virtual clock; then it is dropped, at the table's next change.
struct BlobURLEntry {
  std::vector<uint8_t> bytes;
  std::string type, origin, owner;
  double revokedAt = -1;
};
static std::mutex sBlobURLMutex;
static std::unordered_map<std::string, BlobURLEntry> sBlobURLs;
static constexpr double kBlobURLReleasingMs = 5000;

// GetDataInfo: the URL without its fragment
static std::string BlobURLKey(const std::string& url) { return url.substr(0, url.find('#')); }

// ReleasingTimerHolder::Notify for every entry revoked more than RELEASING_TIMER ago
static void ReleaseRevokedBlobURLs(double now) {
  for (auto it = sBlobURLs.begin(); it != sBlobURLs.end();) {
    if (it->second.revokedAt >= 0 && now - it->second.revokedAt >= kBlobURLReleasingMs) it = sBlobURLs.erase(it);
    else ++it;
  }
}

// __mrtBlobURL(op, ...): bridge.js's URL.createObjectURL and revokeObjectURL, and the loads of blob:
// URLs (fetch, XHR, worker scripts, importScripts, modules):
//   0 (url, bytes, type, origin, owner, now): add an entry (AddDataEntry); `bytes` a Uint8Array
//   1 (url, origin, now): revoke it, when `origin` made it (RemoveDataEntry); whether it did
//   2 (url, origin, alsoIfRevoked, now): [bytes, type] for a load from `origin`, or null (GetDataEntry)
//   3 (url): whether it is there and not revoked (CreateNewURI's revoked flag, taken when parsed)
//   4 (owner, now): revoke every entry of a worker that ended
//   5 (): the bytes the table holds
NATIVE(Global_mrtBlobURL) {
  JS::CallArgs args = JS::CallArgsFromVp(argc, vp);
  int32_t op = args.get(0).isInt32() ? args.get(0).toInt32() : -1;
  std::string url;
  if (op >= 0 && op <= 4 && !ArgUTF8(cx, args.get(1), url)) return false;
  auto number = [&](unsigned i) { return args.get(i).isNumber() ? args.get(i).toNumber() : 0; };
  switch (op) {
    case 0: {
      std::string type, origin, owner;
      if (!ArgUTF8(cx, args.get(3), type) || !ArgUTF8(cx, args.get(4), origin) || !ArgUTF8(cx, args.get(5), owner)) return false;
      BlobURLEntry entry{{}, type, origin, owner};
      {
        JS::AutoCheckCannotGC nogc;
        JSObject* array = JS::Uint8Array::unwrap(args.get(2).isObject() ? &args.get(2).toObject() : nullptr).asObject();
        if (!array) {
          JS_ReportErrorASCII(cx, "__mrtBlobURL: bytes must be a Uint8Array");
          return false;
        }
        size_t length;
        bool shared;
        uint8_t* data;
        js::GetUint8ArrayLengthAndData(array, &length, &shared, &data);
        entry.bytes.assign(data, data + length);
      }
      std::lock_guard<std::mutex> lock(sBlobURLMutex);
      ReleaseRevokedBlobURLs(number(6));
      sBlobURLs[url] = std::move(entry);
      args.rval().setUndefined();
      return true;
    }
    case 1: {
      std::string origin;
      if (!ArgUTF8(cx, args.get(2), origin)) return false;
      std::lock_guard<std::mutex> lock(sBlobURLMutex);
      double now = number(3);
      ReleaseRevokedBlobURLs(now);
      auto it = sBlobURLs.find(BlobURLKey(url));
      bool removed = it != sBlobURLs.end() && it->second.revokedAt < 0 && it->second.origin == origin;
      if (removed) it->second.revokedAt = now;
      args.rval().setBoolean(removed);
      return true;
    }
    case 2: {
      std::string origin;
      if (!ArgUTF8(cx, args.get(2), origin)) return false;
      bool alsoIfRevoked = JS::ToBoolean(args.get(3));
      double now = number(4);
      std::vector<uint8_t> bytes;
      std::string type;
      {
        std::lock_guard<std::mutex> lock(sBlobURLMutex);
        auto it = sBlobURLs.find(BlobURLKey(url));
        // the triggering principal must subsume the entry's: here, the same origin
        if (it == sBlobURLs.end() || it->second.origin != origin ||
            (it->second.revokedAt >= 0 && (!alsoIfRevoked || now - it->second.revokedAt >= kBlobURLReleasingMs))) {
          args.rval().setNull();
          return true;
        }
        bytes = it->second.bytes;
        type = it->second.type;
      }
      JS::RootedObject array(cx, JS::Uint8Array::create(cx, bytes.size()).asObject());
      if (!array) return false;
      {
        JS::AutoCheckCannotGC nogc;
        bool shared;
        uint8_t* data = JS_GetUint8ArrayData(array, &shared, nogc);
        if (!bytes.empty()) memcpy(data, bytes.data(), bytes.size());
      }
      JS::RootedValueArray<2> pair(cx);
      pair[0].setObject(*array);
      if (!NewString(cx, type.data(), type.size(), pair[1])) return false;
      JSObject* result = JS::NewArrayObject(cx, pair);
      if (!result) return false;
      args.rval().setObject(*result);
      return true;
    }
    case 3: {
      std::lock_guard<std::mutex> lock(sBlobURLMutex);
      auto it = sBlobURLs.find(BlobURLKey(url));
      args.rval().setBoolean(it != sBlobURLs.end() && it->second.revokedAt < 0);
      return true;
    }
    case 4: {
      std::lock_guard<std::mutex> lock(sBlobURLMutex);
      double now = number(2);
      ReleaseRevokedBlobURLs(now);
      for (auto& [key, entry] : sBlobURLs) {
        if (entry.owner == url && entry.revokedAt < 0) entry.revokedAt = now;
      }
      args.rval().setUndefined();
      return true;
    }
    case 5: {
      std::lock_guard<std::mutex> lock(sBlobURLMutex);
      double total = 0;
      for (auto& [key, entry] : sBlobURLs) total += entry.bytes.capacity() + key.capacity() + sizeof(entry);
      args.rval().setDouble(total);
      return true;
    }
  }
  JS_ReportErrorASCII(cx, "__mrtBlobURL: unknown op");
  return false;
}

// ---- CompressionStream and DecompressionStream (dom/compression) ----
//
// Gecko's format algorithms (FormatZlib.cpp, FormatBrotli.cpp): the same libraries, parameters and
// 16 KB output buffers, each buffer moved into its chunk's ArrayBuffer as nsJSUtils::
// MoveBufferAsUint8Array does (so a 2-byte chunk holds 16 KB, as in Firefox). The transform stream
// around them is prelude.js's. zstd is not built: release Firefox refuses it
// (dom.compression_streams.zstd.enabled is false), and prelude.js throws Gecko's TypeError for it.
enum class CompressionFormat { Deflate, Deflate_raw, Gzip, Brotli };

// One stream's state: TransformerAlgorithmsBase's subclasses, by format and direction.
struct CompressionAlgorithms {
  CompressionFormat mFormat;
  bool mDecompress;
  // DecompressionStreamAlgorithms::mObservedStreamEnd
  bool mObservedStreamEnd = false;
  z_stream mZStream{};
  BrotliEncoderState* mEncoder = nullptr;
  BrotliDecoderState* mDecoder = nullptr;
  ~CompressionAlgorithms() {
    if (mFormat != CompressionFormat::Brotli) {
      if (mDecompress) inflateEnd(&mZStream);
      else deflateEnd(&mZStream);
    }
    if (mEncoder) BrotliEncoderDestroyInstance(mEncoder);
    if (mDecoder) BrotliDecoderDestroyInstance(mDecoder);
  }
};

// The handle's reflector owns the algorithms (Gecko: the TransformStream's algorithms, freed with it).
static void CompressionAlgorithms_finalize(JS::GCContext* gcx, JSObject* obj) {
  delete JS::GetMaybePtrFromReservedSlot<CompressionAlgorithms>(obj, 0);
}
static const JSClassOps kCompressionAlgorithmsOps = {nullptr, nullptr, nullptr, nullptr, nullptr,
                                                     nullptr, CompressionAlgorithms_finalize, nullptr, nullptr, nullptr};
static const JSClass kCompressionAlgorithmsClass = {"CompressionAlgorithms",
                                                    JSCLASS_HAS_RESERVED_SLOTS(1) | JSCLASS_FOREGROUND_FINALIZE,
                                                    &kCompressionAlgorithmsOps};

// FormatZlib.cpp's ZLibWindowBits
static int ZLibWindowBits(CompressionFormat format) {
  switch (format) {
    case CompressionFormat::Deflate: return 15;
    case CompressionFormat::Deflate_raw: return -15;
    case CompressionFormat::Gzip: return 31;
    default: return 0;
  }
}

static const size_t kCompressionBufferSize = 16384;

// nsJSUtils::MoveBufferAsUint8Array, appended to `out`
static bool AppendCompressionChunk(JSContext* cx, JS::UniqueChars& buffer, size_t written, JS::MutableHandleVector<JSObject*> out) {
  mozilla::UniquePtr<void, JS::FreePolicy> contents(buffer.release());
  JS::RootedObject arrayBuffer(cx, JS::NewArrayBufferWithContents(cx, written, std::move(contents)));
  if (!arrayBuffer) return false;
  JS::RootedObject view(cx, JS_NewUint8ArrayWithBuffer(cx, arrayBuffer, 0, int64_t(written)));
  return view && out.append(view);
}

// The algorithms' TypeErrors (ErrorResult::ThrowTypeError): the message, which prelude.js throws as
// a TypeError
static thread_local std::string sCompressionError;
static bool ThrowCompressionError(JSContext* cx, const std::string& message) {
  sCompressionError = message;
  return false;
}

// ZLibCompressionStreamAlgorithms::Compress and BrotliCompressionStreamAlgorithms::Compress
static bool CompressionCompress(JSContext* cx, CompressionAlgorithms* alg, const std::vector<uint8_t>& input, bool flush,
                                JS::MutableHandleVector<JSObject*> out) {
  if (alg->mFormat == CompressionFormat::Brotli) {
    size_t inputLength = input.size();
    const uint8_t* inputBuffer = input.data();
    do {
      JS::UniqueChars buffer(static_cast<char*>(JS_malloc(cx, kCompressionBufferSize)));
      if (!buffer) return ThrowCompressionError(cx, "Out of memory");
      size_t outputLength = kCompressionBufferSize;
      uint8_t* outputBuffer = reinterpret_cast<uint8_t*>(buffer.get());
      if (!BrotliEncoderCompressStream(alg->mEncoder, flush ? BROTLI_OPERATION_FINISH : BROTLI_OPERATION_PROCESS,
                                       &inputLength, &inputBuffer, &outputLength, &outputBuffer, nullptr)) {
        return ThrowCompressionError(cx, "Unexpected compression error");
      }
      size_t written = kCompressionBufferSize - outputLength;
      if (written > 0 && !AppendCompressionChunk(cx, buffer, written, out)) return false;
    } while (BrotliEncoderHasMoreOutput(alg->mEncoder));
    return true;
  }
  z_stream& zs = alg->mZStream;
  zs.avail_in = input.size();
  zs.next_in = const_cast<uint8_t*>(input.data());
  do {
    JS::UniqueChars buffer(static_cast<char*>(JS_malloc(cx, kCompressionBufferSize)));
    if (!buffer) return ThrowCompressionError(cx, "Out of memory");
    zs.avail_out = kCompressionBufferSize;
    zs.next_out = reinterpret_cast<uint8_t*>(buffer.get());
    int err = deflate(&zs, flush ? Z_FINISH : Z_NO_FLUSH);
    if (err != Z_OK && err != Z_STREAM_END && err != Z_BUF_ERROR) return ThrowCompressionError(cx, "Unexpected compression error");
    size_t written = kCompressionBufferSize - zs.avail_out;
    if (!written) break;
    if (!AppendCompressionChunk(cx, buffer, written, out)) return false;
  } while (zs.avail_out == 0);
  return true;
}

// ZLibDecompressionStreamAlgorithms::Decompress and BrotliDecompressionStreamAlgorithms::Decompress:
// whether the input was fully consumed
static bool CompressionDecompress(JSContext* cx, CompressionAlgorithms* alg, const std::vector<uint8_t>& input, bool flush,
                                  JS::MutableHandleVector<JSObject*> out, bool* fullyConsumed) {
  if (alg->mFormat == CompressionFormat::Brotli) {
    size_t inputLength = input.size();
    const uint8_t* inputBuffer = input.data();
    do {
      JS::UniqueChars buffer(static_cast<char*>(JS_malloc(cx, kCompressionBufferSize)));
      if (!buffer) return ThrowCompressionError(cx, "Out of memory");
      size_t outputLength = kCompressionBufferSize;
      uint8_t* outputBuffer = reinterpret_cast<uint8_t*>(buffer.get());
      BrotliDecoderResult rv = BrotliDecoderDecompressStream(alg->mDecoder, &inputLength, &inputBuffer, &outputLength, &outputBuffer, nullptr);
      switch (rv) {
        case BROTLI_DECODER_RESULT_ERROR:
          return ThrowCompressionError(cx, std::string("Brotli decompression error: ") +
                                               BrotliDecoderErrorString(BrotliDecoderGetErrorCode(alg->mDecoder)));
        case BROTLI_DECODER_RESULT_SUCCESS:
          alg->mObservedStreamEnd = true;
          break;
        case BROTLI_DECODER_RESULT_NEEDS_MORE_INPUT:
        case BROTLI_DECODER_RESULT_NEEDS_MORE_OUTPUT:
          break;
        default:
          return ThrowCompressionError(cx, "Unexpected decompression error");
      }
      size_t written = kCompressionBufferSize - outputLength;
      if (written > 0 && !AppendCompressionChunk(cx, buffer, written, out)) return false;
    } while (BrotliDecoderHasMoreOutput(alg->mDecoder));
    *fullyConsumed = inputLength == 0;
    return true;
  }
  z_stream& zs = alg->mZStream;
  zs.avail_in = input.size();
  zs.next_in = const_cast<uint8_t*>(input.data());
  do {
    JS::UniqueChars buffer(static_cast<char*>(JS_malloc(cx, kCompressionBufferSize)));
    if (!buffer) return ThrowCompressionError(cx, "Out of memory");
    zs.avail_out = kCompressionBufferSize;
    zs.next_out = reinterpret_cast<uint8_t*>(buffer.get());
    int err = inflate(&zs, flush ? Z_FINISH : Z_NO_FLUSH);
    switch (err) {
      case Z_DATA_ERROR:
        return ThrowCompressionError(cx, std::string("The input data is corrupted: ") + (zs.msg ? zs.msg : ""));
      case Z_MEM_ERROR:
        return ThrowCompressionError(cx, "Out of memory");
      case Z_NEED_DICT:
        return ThrowCompressionError(cx, "The stream needs a preset dictionary but such setup is unsupported");
      case Z_STREAM_END:
        alg->mObservedStreamEnd = true;
        break;
      case Z_OK:
      case Z_BUF_ERROR:
        break;
      default:
        return ThrowCompressionError(cx, "Unexpected decompression error");
    }
    size_t written = kCompressionBufferSize - zs.avail_out;
    if (!written) break;
    if (!AppendCompressionChunk(cx, buffer, written, out)) return false;
  } while (zs.avail_out == 0 && !alg->mObservedStreamEnd);
  *fullyConsumed = zs.avail_in == 0;
  return true;
}

// __mrtCompression(op, ...): prelude.js's CompressionStream and DecompressionStream:
//   0 (format, decompress): a handle to new algorithms (ZLib...Algorithms::Init, Brotli...::Init);
//     `format` 0 deflate, 1 deflate-raw, 2 gzip, 3 brotli
//   1 (handle, bytes, flush): the transform (bytes a Uint8Array) or, with `flush`, the flush (bytes
//     undefined): [chunks, fullyConsumed, observedStreamEnd], or the message of the algorithm's
//     TypeError
NATIVE(Global_mrtCompression) {
  JS::CallArgs args = JS::CallArgsFromVp(argc, vp);
  int32_t op = args.get(0).isInt32() ? args.get(0).toInt32() : -1;
  if (op == 0) {
    int32_t format = args.get(1).isInt32() ? args.get(1).toInt32() : -1;
    if (format < 0 || format > 3) {
      JS_ReportErrorASCII(cx, "__mrtCompression: unknown format");
      return false;
    }
    auto alg = std::make_unique<CompressionAlgorithms>();
    alg->mFormat = CompressionFormat(format);
    alg->mDecompress = JS::ToBoolean(args.get(2));
    bool ok;
    if (alg->mFormat == CompressionFormat::Brotli) {
      if (alg->mDecompress) ok = (alg->mDecoder = BrotliDecoderCreateInstance(nullptr, nullptr, nullptr)) != nullptr;
      else ok = (alg->mEncoder = BrotliEncoderCreateInstance(nullptr, nullptr, nullptr)) != nullptr;
    } else if (alg->mDecompress) {
      ok = inflateInit2(&alg->mZStream, ZLibWindowBits(alg->mFormat)) == Z_OK;
    } else {
      ok = deflateInit2(&alg->mZStream, Z_DEFAULT_COMPRESSION, Z_DEFLATED, ZLibWindowBits(alg->mFormat), 8, Z_DEFAULT_STRATEGY) == Z_OK;
    }
    if (!ok) {
      // the constructor's "Not enough memory" (an UnknownError DOMException in Gecko)
      JS_ReportOutOfMemory(cx);
      return false;
    }
    JSObject* handle = JS_NewObject(cx, &kCompressionAlgorithmsClass);
    if (!handle) return false;
    JS::SetReservedSlot(handle, 0, JS::PrivateValue(alg.release()));
    args.rval().setObject(*handle);
    return true;
  }
  if (op == 1) {
    JSObject* handle = args.get(1).isObject() ? &args.get(1).toObject() : nullptr;
    if (!handle || JS::GetClass(handle) != &kCompressionAlgorithmsClass) {
      JS_ReportErrorASCII(cx, "__mrtCompression: not a handle");
      return false;
    }
    auto* alg = JS::GetMaybePtrFromReservedSlot<CompressionAlgorithms>(handle, 0);
    bool flush = JS::ToBoolean(args.get(3));
    // the input copied out first: the loop allocates, and a GC may move a small array's inline data
    std::vector<uint8_t> input;
    if (!flush) {
      JS::AutoCheckCannotGC nogc;
      JSObject* array = JS::Uint8Array::unwrap(args.get(2).isObject() ? &args.get(2).toObject() : nullptr).asObject();
      if (!array) {
        JS_ReportErrorASCII(cx, "__mrtCompression: bytes must be a Uint8Array");
        return false;
      }
      size_t length;
      bool shared;
      uint8_t* data;
      js::GetUint8ArrayLengthAndData(array, &length, &shared, &data);
      input.assign(data, data + length);
    }
    JS::RootedVector<JSObject*> chunks(cx);
    bool fullyConsumed = true;
    sCompressionError.clear();
    if (alg->mDecompress ? !CompressionDecompress(cx, alg, input, flush, &chunks, &fullyConsumed)
                         : !CompressionCompress(cx, alg, input, flush, &chunks)) {
      if (sCompressionError.empty()) return false;
      // the algorithm failed: its message, and no chunks (Gecko enqueues none when aRv failed)
      return NewString(cx, sCompressionError.data(), sCompressionError.size(), args.rval());
    }
    JS::RootedValueVector values(cx);
    for (JSObject* chunk : chunks) {
      if (!values.append(JS::ObjectValue(*chunk))) return false;
    }
    JS::RootedValueArray<3> result(cx);
    JSObject* list = JS::NewArrayObject(cx, values);
    if (!list) return false;
    result[0].setObject(*list);
    result[1].setBoolean(fullyConsumed);
    result[2].setBoolean(alg->mObservedStreamEnd);
    JSObject* triple = JS::NewArrayObject(cx, result);
    if (!triple) return false;
    args.rval().setObject(*triple);
    return true;
  }
  JS_ReportErrorASCII(cx, "__mrtCompression: unknown op");
  return false;
}

// __mrtWorker(op, ...): the page's side of its workers (worker.rs), for bridge.js's Worker:
//   0 (config, origin, secure, isolated, at, shared): start a worker; its id. `config` is a JSON string
//     the worker's own __mrt.workerStart reads (its URL, name, type, the owner's bridge context);
//     `shared` makes its global a SharedWorkerGlobalScope.
//   1 (id, data, at): a message for it (data encoded by clone_codec.js), a task at `at`
//   2 (): [id, at] of the worker task due first, or null
//   3 (id): run that worker's next task; what it handed its parent, a JSON array text
//   4 (id): terminate it
NATIVE(Global_mrtWorker) {
  JS::CallArgs args = JS::CallArgsFromVp(argc, vp);
  int32_t op = args.get(0).isInt32() ? args.get(0).toInt32() : -1;
  void* host = tJs->host;
  switch (op) {
    case 0: {
      std::string config, origin;
      if (!ArgUTF8(cx, args.get(1), config) || !ArgUTF8(cx, args.get(2), origin)) return false;
      double at = args.get(5).isNumber() ? args.get(5).toNumber() : 0;
      args.rval().setInt32(mrt_worker_new(host, config.data(), config.size(), origin.data(), origin.size(),
                                          JS::ToBoolean(args.get(3)), JS::ToBoolean(args.get(4)), at,
                                          JS::ToBoolean(args.get(6))));
      return true;
    }
    case 1: {
      std::string data;
      if (!ArgUTF8(cx, args.get(2), data)) return false;
      mrt_worker_post(host, args.get(1).toNumber(), data.data(), data.size(), args.get(3).toNumber());
      args.rval().setUndefined();
      return true;
    }
    case 2: {
      double at = 0;
      int32_t id = mrt_worker_next(host, &at);
      if (id < 0) {
        args.rval().setNull();
        return true;
      }
      JS::RootedValueArray<2> pair(cx);
      pair[0].setInt32(id);
      pair[1].setDouble(at);
      JSObject* array = JS::NewArrayObject(cx, pair);
      if (!array) return false;
      args.rval().setObject(*array);
      return true;
    }
    case 3: {
      const char* out = nullptr;
      size_t len = 0;
      mrt_worker_run(host, args.get(1).toNumber(), &out, &len);
      return NewString(cx, out, len, args.rval());
    }
    case 4:
      mrt_worker_terminate(host, args.get(1).toNumber());
      args.rval().setUndefined();
      return true;
  }
  JS_ReportErrorASCII(cx, "__mrtWorker: unknown op");
  return false;
}

// __mrtWorkerOut(json): what a worker hands its parent at the end of the task (a message, an error
// report, its close), kept by worker.rs until the task ends.
NATIVE(Global_mrtWorkerOut) {
  JS::CallArgs args = JS::CallArgsFromVp(argc, vp);
  std::string s;
  if (!ArgUTF8(cx, args.get(0), s)) return false;
  mrt_worker_out(tJs->host, s.data(), s.size());
  args.rval().setUndefined();
  return true;
}

// __mrtWorkerScope.runScript(source, url, muted): a classic worker script, or one importScripts loaded,
// in the worker's global (WorkerScriptLoader's EvaluateScript). 0 once it ran; the SyntaxError, returned
// and not thrown, when it does not compile (Gecko fails the load then: a plain error event at the
// Worker, or the error importScripts throws); what it throws while running, thrown. `muted`: compiled
// with muted errors, as an imported script of another origin is (mMutedErrorFlag).
NATIVE(WorkerScope_runScript) {
  JS::CallArgs args = JS::CallArgsFromVp(argc, vp);
  std::string src, url;
  if (!ArgUTF8(cx, args.get(0), src) || !ArgUTF8(cx, args.get(1), url)) return false;
  JS::CompileOptions options(cx);
  options.setFileAndLine(url.c_str(), 1);
  options.setMutedErrors(JS::ToBoolean(args.get(2)));
  JS::SourceText<mozilla::Utf8Unit> text;
  if (!text.init(cx, src.data(), src.size(), JS::SourceOwnership::Borrowed)) return false;
  JS::RootedScript script(cx, JS::Compile(cx, options, text));
  if (!script) {
    JS::RootedValue error(cx);
    if (!JS_GetPendingException(cx, &error)) return false;
    JS_ClearPendingException(cx);
    args.rval().set(error);
    return true;
  }
  JS::RootedValue rval(cx);
  if (!JS_ExecuteScript(cx, script, &rval)) return false;
  args.rval().setInt32(0);
  return true;
}

// __mrtDebuggerEval(source): a debugging aid (the string census, GAPS B89), as Gecko's worker
// debugger global is one: `source` runs in a new global of its own compartment and zone, with
// `Debugger` defined and this global as `target`; its result as a string, or the exception's text.
static const JSClass kDebuggerGlobalClass = {"MrtDebuggerGlobal", JSCLASS_GLOBAL_FLAGS, &JS::DefaultGlobalClassOps};
NATIVE(Global_mrtDebuggerEval) {
  JS::CallArgs args = JS::CallArgsFromVp(argc, vp);
  std::string src;
  if (!ArgUTF8(cx, args.get(0), src)) return false;
  JS::RootedObject target(cx, JS::CurrentGlobalOrNull(cx));
  JS::RealmOptions options;
  options.creationOptions().setNewCompartmentAndZone();
  JS::RootedObject dbgGlobal(cx, JS_NewGlobalObject(cx, &kDebuggerGlobalClass, nullptr, JS::DontFireOnNewGlobalHook, options));
  if (!dbgGlobal) return false;
  JS::RootedValue result(cx);
  {
    JSAutoRealm ar(cx, dbgGlobal);
    JS::RootedValue targetValue(cx, JS::ObjectValue(*target));
    if (!JS::InitRealmStandardClasses(cx) || !JS_DefineDebuggerObject(cx, dbgGlobal) || !JS_WrapValue(cx, &targetValue) ||
        !JS_DefineProperty(cx, dbgGlobal, "target", targetValue, 0)) {
      return false;
    }
    JS::CompileOptions copts(cx);
    copts.setFileAndLine("debugger-eval", 1);
    JS::SourceText<mozilla::Utf8Unit> text;
    if (!text.init(cx, src.data(), src.size(), JS::SourceOwnership::Borrowed)) return false;
    if (!JS::Evaluate(cx, copts, text, &result)) {
      JS::RootedValue error(cx);
      if (!JS_GetPendingException(cx, &error)) return false;
      JS_ClearPendingException(cx);
      result = error;
    }
    JS::RootedString str(cx, JS::ToString(cx, result));
    if (!str) return false;
    result.setString(str);
  }
  if (!JS_WrapValue(cx, &result)) return false;
  args.rval().set(result);
  return true;
}

static const JSFunctionSpec kWorkerScopeFns[] = {
    JS_FN("runScript", WorkerScope_runScript, 3, 0),
    JS_FS_END};

// The worker's global: its interfaces, the natives its JS half uses, `self`. `kind` is its own
// interface, DedicatedWorkerGlobalScope or SharedWorkerGlobalScope.
static bool InitWorkerGlobal(JSContext* cx, JS::HandleObject global, const char* kind) {
  if (!DefineInterface(cx, global, "EventTarget", SLOT_EVENT_TARGET, -1, nullptr, nullptr, nullptr, nullptr,
                       EventTarget_constructor) ||
      !DefineInterface(cx, global, "WorkerGlobalScope", SLOT_WORKER_GLOBAL_SCOPE, SLOT_EVENT_TARGET, nullptr, nullptr) ||
      !DefineInterface(cx, global, kind, SLOT_WORKER_KIND_SCOPE, SLOT_WORKER_GLOBAL_SCOPE, nullptr, nullptr)) {
    return false;
  }
  // each prototype names its interface for Object.prototype.toString, as the WebIDL bindings do
  for (auto [slot, name] : {std::pair{SLOT_EVENT_TARGET, "EventTarget"}, std::pair{SLOT_WORKER_GLOBAL_SCOPE, "WorkerGlobalScope"},
                            std::pair{SLOT_WORKER_KIND_SCOPE, kind}}) {
    JS::RootedObject proto(cx, &JS::GetReservedSlot(global, slot).toObject());
    JS::RootedId tag(cx, JS::PropertyKey::Symbol(JS::GetWellKnownSymbol(cx, JS::SymbolCode::toStringTag)));
    JS::RootedValue value(cx);
    if (!NewString(cx, name, strlen(name), &value) || !JS_DefinePropertyById(cx, proto, tag, value, JSPROP_READONLY)) return false;
  }
  if (!JS_DefineFunction(cx, global, "__mrtRejectionTask", Global_mrtRejectionTask, 1, 0) ||
      !JS_DefineFunction(cx, global, "__mrtEnterCallback", Global_mrtEnterCallback, 0, 0) ||
      !JS_DefineFunction(cx, global, "__mrtLeaveCallback", Global_mrtLeaveCallback, 0, 0) ||
      !JS_DefineFunction(cx, global, "__mrtBridge", Global_mrtBridge, 1, 0) ||
      !JS_DefineFunction(cx, global, "__mrtModuleScript", Global_mrtModuleScript, 2, 0) ||
      !JS_DefineFunction(cx, global, "__mrtModuleStats", Global_mrtModuleStats, 0, 0) ||
      !JS_DefineFunction(cx, global, "__mrtWorkerOut", Global_mrtWorkerOut, 1, 0) ||
      !JS_DefineFunction(cx, global, "__mrtDebuggerEval", Global_mrtDebuggerEval, 1, 0) ||
      !JS_DefineFunction(cx, global, "__mrtBlobURL", Global_mrtBlobURL, 6, 0) ||
      !JS_DefineFunction(cx, global, "__mrtCompression", Global_mrtCompression, 4, 0)) {
    return false;
  }
  JS::RootedObject scope(cx, JS_NewPlainObject(cx));
  if (!scope || !JS_DefineFunctions(cx, scope, kWorkerScopeFns) ||
      !JS_DefineProperty(cx, global, "__mrtWorkerScope", scope, 0)) {
    return false;
  }
  JS::RootedObject proto(cx, &JS::GetReservedSlot(global, SLOT_WORKER_KIND_SCOPE).toObject());
  if (!JS_SetPrototype(cx, global, proto)) return false;
  JS::RootedObject console(cx, JS_NewPlainObject(cx));
  if (!console || !JS_DefineFunctions(cx, console, kConsoleFns) ||
      !JS_DefineProperty(cx, global, "console", console, 0)) {
    return false;
  }
  JS::RootedValue self(cx, JS::ObjectValue(*global));
  return JS_DefineProperty(cx, global, "self", self, JSPROP_ENUMERATE);
}

MrtJs* mrt_js_new_worker(void* host, const MrtGlobalOptions* worker, int shared) {
  // JS_Init ran when the page's context was made, on the page's thread (mrt_js_new)
  auto js = std::make_unique<MrtJs>();
  js->host = host;
  js->cx = JS_NewContext(JS::DefaultHeapMaxBytes);
  if (!js->cx || !js::UseInternalJobQueues(js->cx) || !JS::InitSelfHostedCode(js->cx)) return nullptr;
  tJs = js.get();
  js->worker = true;
  JSContext* cx = js->cx;
  // as the page's (mrt_js_new); Gecko's workers have no cap either (javascript.options.mem.max, -1)
  JS_SetGCParameter(cx, JSGC_MAX_BYTES, 0xffffffff);
  JS::SetPromiseRejectionTrackerCallback(cx, RejectionTracker, nullptr);
  JS_InitDestroyPrincipalsCallback(cx, DestroyPrincipals);
  JS_SetSecurityCallbacks(cx, &kSecurityCallbacks);
  // the module loader, for a module worker and import() (WorkerModuleLoader)
  JS::SetModuleLoadHook(JS_GetRuntime(cx), HostLoadImportedModule);
  JS::SetModuleMetadataHook(JS_GetRuntime(cx), HostPopulateImportMeta);
  js::SetScriptEnvironmentPreparer(cx, &sEnvironmentPreparer);
  JS::RootedObject global(cx, CreateNativeGlobal(cx, worker, nullptr, shared ? &kSharedWorkerGlobalClass : &kWorkerGlobalClass));
  if (!global) return nullptr;
  js->global = std::make_unique<JS::PersistentRootedObject>(cx, global);
  JS_AddExtraGCRootsTracer(cx, TracePreserved, js.get());
  JS_AddWeakPointerZonesCallback(cx, SweepCache, js.get());
  JS::EnterRealm(cx, global);
  if (!InitWorkerGlobal(cx, global, shared ? "SharedWorkerGlobalScope" : "DedicatedWorkerGlobalScope") || !DefineWindowSecurityProperties(cx, global, worker->cross_origin_isolated != 0)) {
    fprintf(stderr, "mrt-js: setting up the worker's global failed\n");
    return nullptr;
  }
  return js.release();
}

// ---- Same-origin frames (DESIGN, "Same-origin frames: the build plan", S2 and S4) ----
//
// A frame element inserted into a document that has a window gets a window of its own: its initial
// about:blank document and a global with the parent's origin, in the page's zone and compartment
// (SelectZone), set up as the page's (InitGlobal, then prelude.js and bridge.js, which share their
// per-node state and the event loop with the page's through __mrtShared). Gecko: nsGenericHTMLFrameElement::
// BindToTree, nsFrameLoader::LoadFrame and the docshell's initial document. Removing the element
// closes the window and every window inside it (nsFrameLoader::Destroy).

static void ReportPending(MrtJs* js);

void mrt_js_set_window_script(MrtJs* js, int32_t which, const char* src, size_t len) {
  (which == 0 ? js->preludeSource : js->bridgeSource).assign(src, len);
}

// Evaluate `src` in the current realm, reporting what it throws.
static bool EvaluateInWindow(JSContext* cx, const std::string& src, const char* filename) {
  if (src.empty()) return true;
  JS::CompileOptions options(cx);
  options.setFileAndLine(filename, 1);
  JS::SourceText<mozilla::Utf8Unit> text;
  JS::RootedValue rval(cx);
  if (!text.init(cx, src.data(), src.size(), JS::SourceOwnership::Borrowed) || !JS::Evaluate(cx, options, text, &rval)) {
    tJs->errors++;
    ReportPending(tJs);
    return false;
  }
  return true;
}

// Call `__mrt.<name>(args...)` of `global`, in its realm; what it throws is reported.
static void CallWindowHook(JSContext* cx, JS::HandleObject global, const char* name, const JS::HandleValueArray& args) {
  JSAutoRealm ar(cx, global);
  JS::RootedValue mrt(cx), fn(cx), rval(cx);
  if (!JS_GetProperty(cx, global, "__mrt", &mrt) || !mrt.isObject()) return;
  JS::RootedObject mrtObj(cx, &mrt.toObject());
  if (!JS_GetProperty(cx, mrtObj, name, &fn) || !fn.isObject()) return;
  JS::RootedValueVector wrapped(cx);
  for (size_t i = 0; i < args.length(); i++) {
    JS::RootedValue v(cx, args[i]);
    if (!JS_WrapValue(cx, &v) || !wrapped.append(v)) return;
  }
  if (!JS_CallFunctionValue(cx, mrtObj, fn, wrapped, &rval)) {
    tJs->errors++;
    ReportPending(tJs);
  }
}

static bool IsFrameElement(uint32_t aNode) {
  if (mrt_dom_kind(tJs->host, aNode) != KIND_ELEMENT) return false;
  const char* s;
  size_t len;
  return mrt_dom_name(tJs->host, aNode, &s, &len) == NS_HTML && std::string_view(s, len) == "iframe";
}

// The window of frame element `aFrame`, open, or null.
static MrtJs::FrameWindow* OpenWindowOfFrame(uint32_t aFrame) {
  for (auto& w : tJs->windows)
    if (w.frame == aFrame && !w.closed) return &w;
  return nullptr;
}

// The document at the shadow-including root of `aNode`, if it has a window: where a frame inserted
// under it gets one (Gecko loads frames in any document with a browsing context).
static uint32_t WindowDocumentOfTree(uint32_t aNode) {
  while (uint32_t parent = mrt_dom_link(tJs->host, aNode, LINK_SHADOW_INCLUDING_PARENT)) aNode = parent;
  return mrt_dom_kind(tJs->host, aNode) == KIND_DOCUMENT && WindowGlobalForDoc(aNode) ? aNode : 0;
}

// A window for frame `aFrame` with document `aDoc` and a global of `aOrigin` (its own compartment
// unless a window of that origin exists, SelectZone), the bindings' JS half evaluated in it. A root
// window (no frame here) is laid out at `aWidth` by `aHeight`; a frame's size comes from its parent's
// layout.
static JSObject* CreateWindowGlobal(JSContext* cx, uint32_t aFrame, uint32_t aDoc, const std::string& aOrigin, bool aSecure,
                                    bool aIsolated, int32_t aWidth = 300, int32_t aHeight = 150) {
  MrtJs* js = tJs;
  MrtGlobalOptions options = {aOrigin.data(), aOrigin.size(), aSecure ? 1 : 0, aIsolated ? 1 : 0};
  JS::RootedObject global(cx, CreateNativeGlobal(cx, &options, *js->global));
  if (!global) return nullptr;
  js->windows.push_back({aDoc, aFrame, std::make_unique<JS::PersistentRootedObject>(cx, global), false});
  // its document's layout, before bridge.js reads its viewport
  mrt_dom_window_opened(js->host, aDoc, aFrame, aWidth, aHeight);
  // the page's shared state, for prelude.js and bridge.js
  JS::RootedValue shared(cx);
  {
    JS::RootedObject page(cx, *js->global);
    JSAutoRealm ar(cx, page);
    JS::RootedValue mrt(cx);
    if (!JS_GetProperty(cx, page, "__mrt", &mrt) || !mrt.isObject()) return nullptr;
    JS::RootedObject mrtObj(cx, &mrt.toObject());
    if (!JS_GetProperty(cx, mrtObj, "shared", &shared)) return nullptr;
  }
  JSAutoRealm ar(cx, global);
  if (!JS_WrapValue(cx, &shared) || !InitGlobal(cx, global, aDoc) ||
      !DefineWindowSecurityProperties(cx, global, aIsolated) ||
      !JS_DefineProperty(cx, global, "__mrtShared", shared, 0)) {
    return nullptr;
  }
  EvaluateInWindow(cx, js->preludeSource, "prelude.js");
  EvaluateInWindow(cx, js->bridgeSource, "bridge.js");
  return global;
}

// A frame's window: its initial about:blank, of its parent's origin.
static bool CreateFrameWindow(JSContext* cx, uint32_t aFrame) {
  MrtJs* js = tJs;
  JS::RootedObject parent(cx, WindowGlobalForDoc(mrt_dom_node_document(js->host, aFrame)));
  if (!parent) return true;
  auto* principal = static_cast<MrtPrincipals*>(JS::GetRealmPrincipals(JS::GetObjectRealmOrNull(parent)));
  std::string origin = principal ? principal->origin : std::string("null");
  bool isolated = false;
  {
    JSAutoRealm ar(cx, parent);
    JS::RootedValue v(cx);
    if (JS_GetProperty(cx, parent, "crossOriginIsolated", &v)) isolated = JS::ToBoolean(v);
  }
  uint32_t doc = mrt_dom_create_blank_document(js->host);
  return CreateWindowGlobal(cx, aFrame, doc, origin, JS::GetIsSecureContext(JS::GetObjectRealmOrNull(parent)), isolated) != nullptr;
}

static bool IsImageElement(uint32_t aNode) {
  if (mrt_dom_kind(tJs->host, aNode) != KIND_ELEMENT) return false;
  const char* s;
  size_t len;
  return mrt_dom_name(tJs->host, aNode, &s, &len) == NS_HTML && std::string_view(s, len) == "img";
}

// An <img>'s src was set or removed by script (HTMLImageElement::AfterMaybeChangeAttr and
// AfterSetAttr queue its image task, in or out of the document): the window of its document gets
// __mrt.imageSrcChanged (bridge.js). An image in a document without a window loads nothing
// (Document::ShouldLoadImages).
static void ImageSrcChanged(uint32_t aElement) {
  if (!IsImageElement(aElement)) return;
  JSContext* cx = tJs->cx;
  JS::RootedObject global(cx, WindowGlobalForDoc(mrt_dom_node_document(tJs->host, aElement)));
  JS::RootedValue el(cx);
  if (!global || !Wrap(cx, aElement, &el)) return;
  CallWindowHook(cx, global, "imageSrcChanged", JS::HandleValueArray(el));
}

// After `aRoot` was inserted: every frame element in its subtree (shadow trees included) that is in a
// document with a window gets one, then its parent window's __mrt.frameInserted (the load event of
// the initial about:blank, or the frame's src or srcdoc load). Each <img> in it goes to the window's
// __mrt.imageInserted: the parser's and innerHTML's images load from here, as their src was set
// without script (Gecko queues their image task when the parser sets src).
static void FramesInserted(JSContext* cx, uint32_t aRoot) {
  if (!WindowDocumentOfTree(aRoot)) return;
  std::vector<uint32_t> inserted, images;
  for (uint32_t n = aRoot; n; n = NextInSubtree(n, aRoot)) {
    if (IsFrameElement(n) && !OpenWindowOfFrame(n) && CreateFrameWindow(cx, n)) inserted.push_back(n);
    else if (IsImageElement(n)) images.push_back(n);
  }
  if (!images.empty()) {
    JS::RootedObject global(cx, WindowGlobalForDoc(mrt_dom_node_document(tJs->host, aRoot)));
    JS::RootedValue el(cx);
    for (uint32_t image : images) {
      if (global && Wrap(cx, image, &el)) CallWindowHook(cx, global, "imageInserted", JS::HandleValueArray(el));
    }
  }
  for (uint32_t frame : inserted) {
    JS::RootedObject parent(cx, WindowGlobalForDoc(mrt_dom_node_document(tJs->host, frame)));
    JS::RootedValue el(cx);
    if (!parent || !Wrap(cx, frame, &el)) continue;
    CallWindowHook(cx, parent, "frameInserted", JS::HandleValueArray(el));
  }
}

// A frame element's src or srcdoc changed while it has a window: it navigates (Gecko's
// nsGenericHTMLFrameElement::AfterSetAttr calls LoadSrc): the parent window's __mrt.frameSrcChanged.
// Its name, set or removed: the frame's browsing-context name becomes it (AfterSetAttr's
// BrowsingContext::SetName): the parent window's __mrt.frameNameChanged.
static void FrameAttributeChanged(uint32_t aElement, const std::string& aName) {
  if (!IsFrameElement(aElement) || !OpenWindowOfFrame(aElement)) return;
  JSContext* cx = tJs->cx;
  JS::RootedObject parent(cx, WindowGlobalForDoc(mrt_dom_node_document(tJs->host, aElement)));
  JS::RootedValue el(cx);
  if (!parent || !Wrap(cx, aElement, &el)) return;
  CallWindowHook(cx, parent, aName == "name" ? "frameNameChanged" : "frameSrcChanged", JS::HandleValueArray(el));
}

// Close window `aIndex` and the windows of the frames in its document.
static void CloseWindow(size_t aIndex) {
  MrtJs* js = tJs;
  if (js->windows[aIndex].closed) return;
  js->windows[aIndex].closed = true;
  uint32_t doc = js->windows[aIndex].doc;
  mrt_dom_window_closed(js->host, doc);
  for (size_t i = 0; i < js->windows.size(); i++) {
    if (!js->windows[i].closed && js->windows[i].frame && mrt_dom_node_document(js->host, js->windows[i].frame) == doc) CloseWindow(i);
  }
}

// Before `aChild` is removed: the windows of the frame elements in its subtree, to close after.
static std::vector<size_t> FramesIn(uint32_t aChild) {
  std::vector<size_t> out;
  for (size_t i = 0; i < tJs->windows.size(); i++) {
    if (tJs->windows[i].closed) continue;
    for (uint32_t n = tJs->windows[i].frame; n; n = mrt_dom_link(tJs->host, n, LINK_SHADOW_INCLUDING_PARENT)) {
      if (n == aChild) {
        out.push_back(i);
        break;
      }
    }
  }
  return out;
}

// The frames of the window whose document is `aDoc`, in tree order (window.frames; frames in shadow
// trees are not in it).
static std::vector<MrtJs::FrameWindow*> ChildWindows(uint32_t aDoc) {
  std::vector<MrtJs::FrameWindow*> out;
  if (tJs->windows.empty()) return out;
  for (uint32_t n = aDoc; n; ) {
    if (IsFrameElement(n))
      if (auto* w = OpenWindowOfFrame(n)) out.push_back(w);
    // the next node in tree order, not into shadow trees
    if (uint32_t c = FirstChild(n)) { n = c; continue; }
    while (n && n != aDoc && !NextSibling(n)) n = ParentNode(n);
    if (!n || n == aDoc) break;
    n = NextSibling(n);
  }
  return out;
}

static bool SetGlobal(JSContext* cx, JSObject* aGlobal, JS::MutableHandleValue rval) {
  if (!aGlobal) {
    rval.setNull();
    return true;
  }
  rval.setObject(*aGlobal);
  return JS_WrapValue(cx, rval);
}

// __mrtWindows(op, ...): the window relations the prelude defines window.parent, top, frameElement,
// frames and length, and the frame element's contentWindow and contentDocument with. The current
// window is the caller's global. op 0 (element): its window or null; 1 (element): its document or
// null; 2: the parent window (the window itself at the top); 3: the frame element or null; 4: the
// frames, an array in tree order; 5 (window): whether it is closed; 6 (element, html): the frame
// navigates to a document parsed from `html`, in the same window (from its initial about:blank, as
// Gecko reuses the inner window), returns the document; 7: the top window; 8 (document, parsed): the
// document takes the parsed one's compatibility mode (document.write); 9 (document): its frames as
// parsed get their windows and loads, as if inserted now (Gecko's parser inserts each, and its
// BindToTree starts its load); 10 (element, html, origin, secure, isolated): the frame navigates to a
// document of another origin, in a new window of that origin, returned; 11 (window, top): its parent,
// or its top, as the current window sees it; 12 (window): the window as the current window sees it;
// 13 (window, index): its frame `index` as the current window sees it, or undefined; 14 (html, origin,
// secure, isolated): a root window, a frame whose embedder is in another runtime, returned.
// Ops 0, 2, 4, 7, 11 and 12 and window[i] give a window of another origin as the current window's
// cross-origin view of it (ViewOfWindow); op 1 gives null for its document.
static bool Global_mrtWindows(JSContext* cx, unsigned argc, JS::Value* vp) {
  JS::CallArgs args = JS::CallArgsFromVp(argc, vp);
  MrtJs* js = tJs;
  int32_t op = args.get(0).isInt32() ? args.get(0).toInt32() : -1;
  JSObject* current = JS::CurrentGlobalOrNull(cx);
  MrtJs::FrameWindow* self = FrameWindowOf(current);
  switch (op) {
    case 0:
    case 1: {
      uint32_t el = NodeOf(args.get(1));
      MrtJs::FrameWindow* w = el ? OpenWindowOfFrame(el) : nullptr;
      if (!w) {
        args.rval().setNull();
        return true;
      }
      if (op == 0) return ViewOfWindow(cx, *w->global, args.rval());
      // a cross-origin frame's document is null to its embedder (HTMLIFrameElement's contentDocument)
      if (current && !SameOriginGlobals(current, *w->global)) {
        args.rval().setNull();
        return true;
      }
      return Wrap(cx, w->doc, args.rval());
    }
    case 2:
    case 7: {
      JSObject* g = current;
      // (a root window, whose embedder is in another process, is its own parent and top here;
      // bridge.js answers them with the embedder's cross-origin view)
      for (MrtJs::FrameWindow* w = self; w && !w->closed && w->frame; w = FrameWindowOf(g)) {
        JSObject* parent = WindowGlobalForDoc(mrt_dom_node_document(js->host, w->frame));
        if (!parent) break;
        g = parent;
        if (op == 2) break;
      }
      return ViewOfWindow(cx, g, args.rval());
    }
    case 3: {
      if (!self || self->closed || !self->frame) {
        args.rval().setNull();
        return true;
      }
      // null when the embedder is of another origin (Window::GetFrameElement's subsumes check)
      JSObject* embedder = WindowGlobalForDoc(mrt_dom_node_document(js->host, self->frame));
      if (!embedder || !SameOriginGlobals(current, embedder)) {
        args.rval().setNull();
        return true;
      }
      return Wrap(cx, self->frame, args.rval());
    }
    case 4: {
      uint32_t doc = self ? self->doc : 1;
      std::vector<MrtJs::FrameWindow*> frames = ChildWindows(doc);
      JS::RootedObject array(cx, JS::NewArrayObject(cx, frames.size()));
      if (!array) return false;
      for (size_t i = 0; i < frames.size(); i++) {
        JS::RootedValue v(cx);
        if (!ViewOfWindow(cx, *frames[i]->global, &v) || !JS_SetElement(cx, array, i, v)) return false;
      }
      args.rval().setObject(*array);
      return true;
    }
    case 5: {
      if (!args.get(1).isObject()) {
        args.rval().setBoolean(false);
        return true;
      }
      // a window of another origin comes through the engine's wrapper between compartments
      JSObject* target = js::UncheckedUnwrap(&args.get(1).toObject());
      MrtJs::FrameWindow* w = FrameWindowOf(target);
      args.rval().setBoolean(w && w->closed);
      return true;
    }
    case 6: {
      uint32_t el = NodeOf(args.get(1));
      MrtJs::FrameWindow* w = el ? OpenWindowOfFrame(el) : nullptr;
      std::string html;
      if (!w || !ArgUTF8(cx, args.get(2), html)) {
        args.rval().setNull();
        return true;
      }
      uint32_t doc = mrt_dom_parse_frame_document(js->host, html.data(), html.size());
      mrt_dom_window_document_changed(js->host, w->doc, doc);
      w->doc = doc;
      JS::RootedObject global(cx, *w->global);
      JS::RootedValue docValue(cx);
      {
        JSAutoRealm ar(cx, global);
        if (!Wrap(cx, doc, &docValue) || !JS_DefineProperty(cx, global, "document", docValue, JSPROP_ENUMERATE | JSPROP_READONLY)) return false;
      }
      args.rval().set(docValue);
      return JS_WrapValue(cx, args.rval());
    }
    case 8: {
      // (document, parsed): the document takes the compatibility mode of what was written
      uint32_t doc = NodeOf(args.get(1)), parsed = NodeOf(args.get(2));
      if (doc && parsed) mrt_dom_set_document_mode(js->host, doc, mrt_dom_document_mode(js->host, parsed));
      args.rval().setUndefined();
      return true;
    }
    case 9: {
      if (uint32_t doc = NodeOf(args.get(1))) FramesInserted(cx, doc);
      args.rval().setUndefined();
      return true;
    }
    case 10: {
      // (element, html, origin, secure, isolated): the frame navigates to a document of another
      // origin of this runtime's site, in a new window of that origin (Gecko makes a new inner window
      // for a cross-origin navigation; SPEC decision 19): returns the new window
      uint32_t el = NodeOf(args.get(1));
      MrtJs::FrameWindow* w = el ? OpenWindowOfFrame(el) : nullptr;
      std::string html, origin;
      if (!w || !ArgUTF8(cx, args.get(2), html) || !ArgUTF8(cx, args.get(3), origin)) {
        args.rval().setNull();
        return true;
      }
      size_t index = size_t(w - js->windows.data());
      uint32_t doc = mrt_dom_parse_frame_document(js->host, html.data(), html.size());
      JS::RootedObject global(cx, CreateWindowGlobal(cx, el, doc, origin, JS::ToBoolean(args.get(4)), JS::ToBoolean(args.get(5))));
      if (!global) return false;
      // the old window, the frame's initial about:blank, is gone
      CloseWindow(index);
      return SetGlobal(cx, global, args.rval());
    }
    case 11: {
      // (window, top): that window's parent, or its top, as the current window sees it (bridge.js's
      // cross-origin views answer parent and top so)
      if (!args.get(1).isObject()) {
        args.rval().setNull();
        return true;
      }
      JSObject* g = js::UncheckedUnwrap(&args.get(1).toObject());
      for (MrtJs::FrameWindow* w = FrameWindowOf(g); w && !w->closed && w->frame; w = FrameWindowOf(g)) {
        JSObject* parent = WindowGlobalForDoc(mrt_dom_node_document(js->host, w->frame));
        if (!parent) break;
        g = parent;
        if (!JS::ToBoolean(args.get(2))) break;
      }
      return ViewOfWindow(cx, g, args.rval());
    }
    case 12:
      // (window): that window as the current window sees it (a message's source)
      return ViewOfWindow(cx, args.get(1).isObject() ? js::UncheckedUnwrap(&args.get(1).toObject()) : nullptr, args.rval());
    case 13: {
      // (window, index): that window's frame `index` as the current window sees it, or undefined
      // (a cross-origin window's indexed properties, CrossOriginGetOwnPropertyHelper)
      JSObject* g = args.get(1).isObject() ? js::UncheckedUnwrap(&args.get(1).toObject()) : nullptr;
      MrtJs::FrameWindow* w = g ? FrameWindowOf(g) : nullptr;
      std::vector<MrtJs::FrameWindow*> frames = ChildWindows(w ? w->doc : 1);
      double index = args.get(2).isNumber() ? args.get(2).toNumber() : -1;
      if (!g || !(index >= 0) || index >= frames.size()) {
        args.rval().setUndefined();
        return true;
      }
      return ViewOfWindow(cx, *frames[size_t(index)]->global, args.rval());
    }
    case 14: {
      // (html, origin, secure, isolated, width, height): a root window, a frame whose embedder is in
      // another runtime, of this runtime's site (Firefox runs all the tab's frames of a site in its
      // process, SPEC decision 19): a document parsed from `html` in a new window of `origin`, laid out
      // at the frame's size, returned
      std::string html, origin;
      if (!ArgUTF8(cx, args.get(1), html) || !ArgUTF8(cx, args.get(2), origin)) {
        args.rval().setNull();
        return true;
      }
      uint32_t doc = mrt_dom_parse_frame_document(js->host, html.data(), html.size());
      int32_t width = args.get(5).isNumber() ? int32_t(args.get(5).toNumber()) : 300;
      int32_t height = args.get(6).isNumber() ? int32_t(args.get(6).toNumber()) : 150;
      JS::RootedObject global(cx, CreateWindowGlobal(cx, 0, doc, origin, JS::ToBoolean(args.get(3)), JS::ToBoolean(args.get(4)), width, height));
      if (!global) return false;
      return SetGlobal(cx, global, args.rval());
    }
  }
  JS_ReportErrorASCII(cx, "__mrtWindows: unknown op");
  return false;
}

// window.postMessage(message, options), as Gecko's Window::PostMessageMoz, a native: the message's
// source and origin are those of the window whose script called it (the incumbent global,
// JS::GetScriptedCallerGlobal), which a JS function cannot know. The window it was called on (this
// native's realm) gets the caller in its __mrt.postMessageFrom (bridge.js).
static bool Global_mrtPostMessage(JSContext* cx, unsigned argc, JS::Value* vp) {
  JS::CallArgs args = JS::CallArgsFromVp(argc, vp);
  JS::RootedObject target(cx, JS::CurrentGlobalOrNull(cx));
  JSObject* callerGlobal = JS::GetScriptedCallerGlobal(cx);
  JS::RootedValue caller(cx, JS::ObjectValue(callerGlobal ? *callerGlobal : *target));
  JS::RootedValue mrt(cx), fn(cx);
  if (!JS_WrapValue(cx, &caller) || !JS_GetProperty(cx, target, "__mrt", &mrt) || !mrt.isObject()) return false;
  JS::RootedObject mrtObj(cx, &mrt.toObject());
  if (!JS_GetProperty(cx, mrtObj, "postMessageFrom", &fn)) return false;
  JS::RootedValueArray<4> callArgs(cx);
  callArgs[0].set(caller);
  callArgs[1].set(args.get(0));
  callArgs[2].set(args.get(1));
  callArgs[3].set(args.get(2));
  return JS_CallFunctionValue(cx, mrtObj, fn, callArgs, args.rval());
}

// __mrtWithIncumbent(impl, name, length): a native function that calls impl with the same `this` and
// with the window whose script called it (the incumbent global, JS::GetScriptedCallerGlobal; undefined
// when no script is running) in front of its arguments. addEventListener keeps it with the listener,
// as Gecko's CallbackObject keeps its incumbent global, for window.event (WindowFromListener).
static bool WithIncumbent_Call(JSContext* cx, unsigned argc, JS::Value* vp) {
  JS::CallArgs args = JS::CallArgsFromVp(argc, vp);
  JS::RootedValue impl(cx, js::GetFunctionNativeReserved(&args.callee(), 0));
  JSObject* callerGlobal = JS::GetScriptedCallerGlobal(cx);
  JS::RootedValueVector callArgs(cx);
  if (!callArgs.append(callerGlobal ? JS::ObjectValue(*callerGlobal) : JS::UndefinedValue())) return false;
  if (!JS_WrapValue(cx, callArgs[0])) return false;
  for (unsigned i = 0; i < args.length(); i++) {
    if (!callArgs.append(args[i])) return false;
  }
  return JS::Call(cx, args.thisv(), impl, JS::HandleValueArray(callArgs), args.rval());
}
static bool Global_mrtWithIncumbent(JSContext* cx, unsigned argc, JS::Value* vp) {
  JS::CallArgs args = JS::CallArgsFromVp(argc, vp);
  if (!args.get(0).isObject() || !JS::IsCallable(&args[0].toObject()) || !args.get(1).isString()) {
    JS_ReportErrorASCII(cx, "__mrtWithIncumbent: (function, name, length)");
    return false;
  }
  JS::RootedString nameString(cx, args[1].toString());
  JS::UniqueChars name = JS_EncodeStringToUTF8(cx, nameString);
  if (!name) return false;
  unsigned length = args.get(2).isInt32() ? unsigned(args[2].toInt32()) : 0;
  JSFunction* fn = js::NewFunctionWithReserved(cx, WithIncumbent_Call, length, 0, name.get());
  if (!fn) return false;
  js::SetFunctionNativeReserved(JS_GetFunctionObject(fn), 0, args[0]);
  args.rval().setObject(*JS_GetFunctionObject(fn));
  return true;
}

// document.all's object (Gecko's HTMLAllCollection binding): a proxy whose class emulates undefined,
// as Codegen.py gives HTMLAllCollection's DOM proxy class JSCLASS_EMULATES_UNDEFINED. typeof gives
// "undefined", it converts to false and equals null and undefined loosely; every other operation goes
// to its target, a JS Proxy bridge.js builds with the collection's getters and legacy caller.
//
// A family of its own, not js::Wrapper's: EmulatesUndefined unwraps a wrapper and would read the
// target's class. A refusal from the target's traps (a "returned false" code) is given the code Gecko's
// DOMProxyHandler reports, so strict code and Object.defineProperty throw Firefox's messages.
class EmulatesUndefinedHandler : public js::ForwardingProxyHandler {
 public:
  static const char family;
  constexpr EmulatesUndefinedHandler() : js::ForwardingProxyHandler(&family) {}

  bool defineProperty(JSContext* cx, JS::HandleObject proxy, JS::HandleId id, JS::Handle<JS::PropertyDescriptor> desc,
                      JS::ObjectOpResult& result) const override {
    if (!js::ForwardingProxyHandler::defineProperty(cx, proxy, id, desc, result)) return false;
    if (result || result.failureCode() != JSMSG_PROXY_DEFINE_RETURNED_FALSE) return true;
    return id.isInt() ? result.failNoIndexedSetter() : result.failNoNamedSetter();
  }
  bool set(JSContext* cx, JS::HandleObject proxy, JS::HandleId id, JS::HandleValue v, JS::HandleValue receiver,
           JS::ObjectOpResult& result) const override {
    if (!js::ForwardingProxyHandler::set(cx, proxy, id, v, receiver, result)) return false;
    if (result || result.failureCode() != JSMSG_PROXY_DEFINE_RETURNED_FALSE) return true;
    return result.failReadOnly();
  }
  bool delete_(JSContext* cx, JS::HandleObject proxy, JS::HandleId id, JS::ObjectOpResult& result) const override {
    if (!js::ForwardingProxyHandler::delete_(cx, proxy, id, result)) return false;
    if (result || result.failureCode() != JSMSG_PROXY_DELETE_RETURNED_FALSE) return true;
    return result.failCantDelete();
  }
  bool preventExtensions(JSContext* cx, JS::HandleObject proxy, JS::ObjectOpResult& result) const override {
    return result.failCantPreventExtensions();
  }
  bool isExtensible(JSContext* cx, JS::HandleObject proxy, bool* extensible) const override {
    *extensible = true;
    return true;
  }
};
const char EmulatesUndefinedHandler::family = 0;
static const EmulatesUndefinedHandler kEmulatesUndefinedHandler;
static const JSClass kHTMLAllCollectionClass = PROXY_CLASS_DEF("HTMLAllCollection", JSCLASS_HAS_RESERVED_SLOTS(1) | JSCLASS_EMULATES_UNDEFINED);

// __mrtEmulateUndefined(target): document.all's object over target (its prototype is target's)
static bool Global_mrtEmulateUndefined(JSContext* cx, unsigned argc, JS::Value* vp) {
  JS::CallArgs args = JS::CallArgsFromVp(argc, vp);
  if (!args.get(0).isObject()) {
    JS_ReportErrorASCII(cx, "__mrtEmulateUndefined: (target)");
    return false;
  }
  js::ProxyOptions options;
  options.setClass(&kHTMLAllCollectionClass).setLazyProto(true);
  JSObject* proxy = js::NewProxyObject(cx, &kEmulatesUndefinedHandler, args[0], nullptr, options);
  if (!proxy) return false;
  args.rval().setObject(*proxy);
  return true;
}

void mrt_js_shutdown(void) { JS_ShutDown(); }

static void ReportPending(MrtJs* js) {
  JSContext* cx = js->cx;
  JS::ExceptionStack stack(cx);
  if (!JS::StealPendingExceptionStack(cx, &stack)) {
    fprintf(stderr, "mrt-js: uncatchable exception\n");
    return;
  }
  JS::ErrorReportBuilder report(cx);
  if (report.init(cx, stack, JS::ErrorReportBuilder::WithSideEffects)) {
    JS::PrintError(stderr, report, false);
  } else {
    JS_ClearPendingException(cx);
  }
}

// CycleCollectedJSContext::AfterProcessMicrotasks: after each microtask checkpoint IndexedDB's
// transactions are cleaned up (CleanupIDBTransactions): bridge.js's __mrtAfterMicrotasks, which makes
// the transactions of this task inactive and commits the finished ones. Absent without a bridge.
static void AfterProcessMicrotasks(MrtJs* js) {
  JSContext* cx = js->cx;
  JS::RootedObject global(cx, *js->global);
  JS::RootedValue fn(cx);
  if (!JS_GetProperty(cx, global, "__mrtAfterMicrotasks", &fn) || !fn.isObject()) {
    JS_ClearPendingException(cx);
    return;
  }
  JS::RootedValue rval(cx);
  if (!JS_CallFunctionValue(cx, global, fn, JS::HandleValueArray::empty(), &rval)) {
    js->errors++;
    ReportPending(js);
  }
  js::RunJobs(cx);
}

// A microtask checkpoint (CycleCollectedJSContext::PerformMicroTaskCheckPoint): the promise jobs,
// then AfterProcessMicrotasks. At the end of each task (mrt_js_eval) and when a page callback the
// runtime called returns with no other page code on the stack (__mrtLeaveCallback). Does nothing
// while a checkpoint is running.
static void MicrotaskCheckpoint(MrtJs* js) {
  if (js->performingCheckpoint) return;
  JSContext* cx = js->cx;
  // the page's realm for AfterProcessMicrotasks: a listener in a frame's window of another origin
  // calls this from that window's compartment
  JSAutoRealm ar(cx, *js->global);
  js->performingCheckpoint = true;
  js::RunJobs(cx);
  if (JS_IsExceptionPending(cx)) {
    js->errors++;
    ReportPending(js);
  }
  AfterProcessMicrotasks(js);
  js->performingCheckpoint = false;
}

// A source Rust evaluates is page code (a script, a test's) unless it is the runtime's own call
// into the glue: then the page code the glue calls counts itself.
static bool IsPageSource(const char* filename) {
  return strcmp(filename, "driver") != 0 && strcmp(filename, "worker") != 0;
}

// ---- the document's parser (mrt-dom's DocumentParser, driven by Page::parse_step) ----
//
// The page's document is parsed at its scripts' pace: the parser stops after each popped <script>,
// which runs before it goes on. Its changes to the tree are notified as Gecko's tree operations do
// (nsHtml5TreeOperation::Append: ContentAppended per node, to the mutation observers); the reactions
// that run script wait for the parser's next stop here: custom element connected reactions and
// upgrades (Gecko runs them after each operation that queued one), the windows of the frames
// inserted (BindToTree), and each <link>'s sheet load (bridge.js's parsedLink). Defined custom
// elements are upgraded at the stop, not constructed by the parser as Gecko's are.

extern "C" void mrt_js_parser_inserted(uint32_t aParent, uint32_t aChild) {
  if (!tJs || !aParent) return;
  NotifyChildAdded(aParent, aChild, !NextSibling(aChild));
  tJs->parserInserted.push_back(aChild);
}

extern "C" void mrt_js_parser_will_remove(uint32_t aParent, uint32_t aChild) {
  if (!tJs || !aParent) return;
  NotifyContentWillBeRemoved(aParent, aChild);
}

// The parser stopped (at a script or the end): the reactions to what it inserted since its last stop,
// for each node not under another one inserted meanwhile, in the order they were inserted.
extern "C" void mrt_js_parser_stopped() {
  MrtJs* js = tJs;
  if (!js) return;
  JSContext* cx = js->cx;
  std::vector<uint32_t> nodes = std::move(js->parserInserted);
  js->parserInserted.clear();
  std::unordered_set<uint32_t> inserted(nodes.begin(), nodes.end());
  std::vector<uint32_t> roots;
  for (uint32_t n : nodes) {
    bool under = false;
    for (uint32_t p = ParentNode(n); p && !under; p = ParentNode(p)) under = inserted.count(p) != 0;
    if (!under && IsConnected(n)) roots.push_back(n);
  }
  {
    AutoCEReaction ceReaction(cx);
    for (uint32_t r : roots) EnqueueConnectedReactions(r);
  }
  for (uint32_t r : roots) FramesInserted(cx, r);
  JS::RootedObject global(cx, *js->global);
  JS::RootedValue el(cx);
  for (uint32_t r : roots) {
    for (uint32_t n = r; n; n = NextInSubtree(n, r)) {
      const char* name;
      size_t len;
      if (mrt_dom_kind(js->host, n) != KIND_ELEMENT || mrt_dom_name(js->host, n, &name, &len) != NS_HTML ||
          std::string_view(name, len) != "link" || !IsConnected(n))
        continue;
      if (Wrap(cx, n, &el)) CallWindowHook(cx, global, "parsedLink", JS::HandleValueArray(el));
    }
  }
  MicrotaskCheckpoint(js);
  FlushRejections(js);
}

// The parser popped `aScript`: bridge.js's parserScript runs it (or keeps it for later), then a
// microtask checkpoint (the end tag's, and the script's own).
extern "C" void mrt_js_parser_script(uint32_t aScript) {
  MrtJs* js = tJs;
  if (!js) return;
  JSContext* cx = js->cx;
  JS::RootedObject global(cx, *js->global);
  JS::RootedValue el(cx);
  if (Wrap(cx, aScript, &el)) CallWindowHook(cx, global, "parserScript", JS::HandleValueArray(el));
  MicrotaskCheckpoint(js);
  FlushRejections(js);
}

int mrt_js_eval(MrtJs* js, const char* src, size_t len, const char* filename) {
  JSContext* cx = js->cx;
  bool page = IsPageSource(filename);
  JS::CompileOptions options(cx);
  options.setFileAndLine(filename, 1);
  JS::SourceText<mozilla::Utf8Unit> text;
  if (!text.init(cx, src, len, JS::SourceOwnership::Borrowed)) {
    js->errors++;
    ReportPending(js);
    return 0;
  }
  JS::RootedValue rval(cx);
  if (page) js->callbackDepth++;
  bool ok = JS::Evaluate(cx, options, text, &rval);
  if (page) js->callbackDepth--;
  if (!ok) {
    js->errors++;
    ReportPending(js);
  }
  MicrotaskCheckpoint(js);
  FlushRejections(js);
  return ok;
}

int mrt_js_eval_number(MrtJs* js, const char* src, size_t len, const char* filename, double* out) {
  JSContext* cx = js->cx;
  bool page = IsPageSource(filename);
  *out = JS::GenericNaN();
  JS::CompileOptions options(cx);
  options.setFileAndLine(filename, 1);
  JS::SourceText<mozilla::Utf8Unit> text;
  JS::RootedValue rval(cx);
  if (page) js->callbackDepth++;
  bool ok = text.init(cx, src, len, JS::SourceOwnership::Borrowed) && JS::Evaluate(cx, options, text, &rval);
  if (page) js->callbackDepth--;
  if (ok && rval.isNumber()) *out = rval.toNumber();
  if (!ok) {
    js->errors++;
    ReportPending(js);
  }
  MicrotaskCheckpoint(js);
  FlushRejections(js);
  return ok;
}

// Run a driver script and return its value as UTF-8 text (empty if it threw): `*out_len` bytes at
// the result, which stays valid until the next call on this thread.
const char* mrt_js_eval_string(MrtJs* js, const char* src, size_t len, const char* filename, size_t* out_len) {
  static thread_local std::string result;
  JSContext* cx = js->cx;
  result.clear();
  JS::CompileOptions options(cx);
  options.setFileAndLine(filename, 1);
  JS::SourceText<mozilla::Utf8Unit> text;
  JS::RootedValue rval(cx);
  bool ok = text.init(cx, src, len, JS::SourceOwnership::Borrowed) && JS::Evaluate(cx, options, text, &rval) &&
            ArgUTF8(cx, rval, result);
  if (!ok) {
    js->errors++;
    ReportPending(js);
  }
  MicrotaskCheckpoint(js);
  FlushRejections(js);
  *out_len = result.size();
  return result.data();
}

size_t mrt_js_errors(MrtJs* js) { return js->errors; }

// Two GCs, as Firefox's harness does: the first one purges the zone's cached code pools before it
// finalizes the JIT code in them, so an emptied pool (64 KB) is freed only by the next one.
void mrt_js_gc(MrtJs* js) {
  for (int i = 0; i < 2; i++) {
    JS::PrepareForFullGC(js->cx);
    JS::NonIncrementalGC(js->cx, JS::GCOptions::Normal, JS::GCReason::API);
  }
}

// CollectRuntimeStats needs an embedder subclass. Each realm's and zone's stats are tagged with
// the realm or zone in `extra`, so the page's can be picked out.
struct Stats : JS::RuntimeStats {
  Stats() : JS::RuntimeStats(malloc_usable_size_of) {}
  static size_t malloc_usable_size_of(const void* p) { return malloc_usable_size(const_cast<void*>(p)); }
  void initExtraRealmStats(JS::Realm* realm, JS::RealmStats* r, const JS::AutoRequireNoGC&) override { r->extra = realm; }
  void initExtraZoneStats(JS::Zone* zone, JS::ZoneStats* z, const JS::AutoRequireNoGC&) override { z->extra = zone; }
};

// The page going idle, as Firefox's content process does once the user is inactive
// (CCGCScheduler::PokeShrinkingGC): a shrinking GC, which shrinks the nursery to its minimum and
// hands every script source waiting for compression to the helper threads (without it a source
// waits for three more major GCs, which a page in the mini runtime never reaches). The finished
// compressions are attached by the sweep of a later GC, so this waits for the helper threads and
// GCs again until the source cache stops shrinking. Returns the milliseconds it waited.
static size_t SourceCacheBytes(MrtJs* js) {
  Stats stats;
  if (!JS::CollectRuntimeStats(js->cx, &stats, nullptr, false)) return 0;
  return stats.runtime.sharedImmutableStringsCache;
}

size_t mrt_js_idle_gc(MrtJs* js) {
  JS::PrepareForFullGC(js->cx);
  JS::NonIncrementalGC(js->cx, JS::GCOptions::Shrink, JS::GCReason::API);
  size_t before = SourceCacheBytes(js), waited = 0;
  for (int i = 0; i < 20; i++) {
    std::this_thread::sleep_for(std::chrono::milliseconds(100));
    waited += 100;
    JS::PrepareForFullGC(js->cx);
    JS::NonIncrementalGC(js->cx, JS::GCOptions::Shrink, JS::GCReason::API);
    size_t now = SourceCacheBytes(js);
    if (now == before) break;
    before = now;
  }
  return waited;
}

// The interactive page's idle upkeep (realsite --interactive, GAPS A22): one shrinking GC, as
// CCGCScheduler's once the user is inactive, without mrt_js_idle_gc's wait for the source
// compressions, so that it holds the page for the GC only. It decommits the free arenas, which a
// normal GC skips while GCs come often (GCRuntime::shouldDecommit).
void mrt_js_shrink_gc(MrtJs* js) {
  JS::PrepareForFullGC(js->cx);
  JS::NonIncrementalGC(js->cx, JS::GCOptions::Shrink, JS::GCReason::API);
}

static void ToSizes(const JS::ServoSizes& s, MrtJsSizes* out) {
  out->gc_used = s.gcHeapUsed;
  out->gc_unused = s.gcHeapUnused;
  out->gc_admin = s.gcHeapAdmin;
  out->malloc_heap = s.mallocHeap;
  out->non_heap = s.nonHeap;
}

// The whole runtime's memory, not only the page's realm and zone: what `realsite` needs to account
// for its process (GAPS A16). One JSON object, in bytes, grouped by where the bytes physically are:
//   malloc   glibc malloc blocks (measured with malloc_usable_size), by owner
//   chunks   the GC's 1 MiB chunks (tenured arenas and buffer chunks), mapped by SpiderMonkey
//   nursery  the nursery's committed chunks, mapped
//   code     JIT code pools, mapped
// plus the page's realm and zone in the categories Firefox's about:memory uses. Written to `out`
// up to `cap` bytes; returns the length it needed.
size_t mrt_js_memory_detail(MrtJs* js, char* out, size_t cap) {
  Stats stats;
  std::string j;
  auto field = [&j](const char* name, size_t v) {
    char b[96];
    snprintf(b, sizeof(b), "%s\"%s\":%zu", j.back() == '{' ? "" : ",", name, v);
    j += b;
  };
  j = "{";
  if (!JS::CollectRuntimeStats(js->cx, &stats, nullptr, false)) {
    j += "}";
  } else {
    JS::Realm* realm = JS::GetObjectRealmOrNull(*js->global);
    JS::Zone* zone = JS::GetObjectZone(*js->global);
    const JS::RuntimeSizes& rt = stats.runtime;
    size_t sources = rt.scriptSourceInfo.misc;
    for (auto& n : rt.notableScriptSources) sources += n.misc;
    const JS::GCSizes& g = rt.gc;
    size_t storeBuffer = g.storeBufferVals + g.storeBufferCells + g.storeBufferSlots + g.storeBufferWasmAnyRefs +
                         g.storeBufferWholeCells + g.storeBufferGenerics;

    // malloc by owner. The zone's GC buffers are reported as malloc but live in buffer chunks or in
    // their own mappings (large buffers), so they are left out here.
    j += "\"malloc\":{";
    field("atoms_table", rt.atomsTable);
    field("atoms_mark_bitmaps", rt.atomsMarkBitmaps);
    field("self_host_stencil", rt.selfHostStencil);
    field("script_sources", sources);
    field("script_data", rt.scriptData);
    field("shared_immutable_strings", rt.sharedImmutableStringsCache);
    field("uncompressed_source_cache", rt.uncompressedSourceCache);
    field("shared_intl_data", rt.sharedIntlData);
    field("runtime_object", rt.object);
    field("contexts", rt.contexts);
    field("temporary", rt.temporary);
    field("interpreter_stack", rt.interpreterStack);
    field("jit_lazy_link", rt.jitLazyLink);
    field("wasm_runtime", rt.wasmRuntime);
    field("gc_marker", g.marker);
    field("gc_store_buffer", storeBuffer);
    field("gc_nursery_malloced_buffers", g.nurseryMallocedBuffers);
    // realms and zones: the page's and the others (the atoms zone, the self-hosting realm's)
    JS::ServoSizes pageRealm, pageZone, otherRealms, otherZones;
    size_t pageZoneBuffers = 0, otherZoneBuffers = 0;
    for (auto& r : stats.realmStatsVector) {
      JS::ServoSizes& t = r.extra == realm ? pageRealm : otherRealms;
      r.addToServoSizes(&t);
      for (auto& c : r.notableClasses) c.addToServoSizes(&t);
    }
    for (auto& z : stats.zoneStatsVector) {
      JS::ServoSizes& t = z.extra == zone ? pageZone : otherZones;
      z.addToServoSizes(&t);
      for (auto& str : z.notableStrings) str.addToServoSizes(&t);
      (z.extra == zone ? pageZoneBuffers : otherZoneBuffers) += z.gcBuffers.totalSize();
    }
    // the strings of the zones other than the page's (the atoms zone's are the atoms), GC heap and
    // malloc: Firefox's report lists the atoms zone as a zone of its own (GAPS B89)
    size_t otherStrings = 0;
    for (auto& z : stats.zoneStatsVector) {
      if (z.extra == zone) continue;
      otherStrings += z.stringInfo.gcHeapLatin1 + z.stringInfo.gcHeapTwoByte + z.stringInfo.mallocHeapLatin1 + z.stringInfo.mallocHeapTwoByte;
      for (auto& str : z.notableStrings)
        otherStrings += str.gcHeapLatin1 + str.gcHeapTwoByte + str.mallocHeapLatin1 + str.mallocHeapTwoByte;
    }
    field("page_realm", pageRealm.mallocHeap);
    field("page_zone", pageZone.mallocHeap - pageZoneBuffers);
    field("other_realms", otherRealms.mallocHeap);
    field("other_zones", otherZones.mallocHeap - otherZoneBuffers);
    j += "}";

    // the GC heap's chunks, as Firefox's js-main-runtime-gc-heap-committed splits them
    const JS::ZoneStats& zt = stats.zTotals;
    j += ",\"chunks\":{";
    field("total", stats.gcHeapChunkTotal);
    field("decommitted", stats.gcHeapDecommittedPages);
    field("unused_chunks", stats.gcHeapUnusedChunks);
    field("unused_arenas", stats.gcHeapUnusedArenas);
    field("chunk_admin", stats.gcHeapChunkAdmin);
    field("arena_admin", zt.gcHeapArenaAdmin);
    field("unused_gc_things", zt.unusedGCThings.totalSize());
    field("gc_things", stats.gcHeapGCThings);
    field("buffers_used", zt.gcBuffers.usedBytes);
    field("buffers_free", zt.gcBuffers.freeBytes);
    field("buffers_admin", zt.gcBuffers.adminBytes);
    field("buffer_chunks", zt.gcBuffers.totalChunks);
    field("buffer_large_allocs", zt.gcBuffers.largeAllocs);
    j += "}";

    j += ",\"other_zones\":{";
    field("strings", otherStrings);
    j += "}";

    j += ",\"mapped\":{";
    field("nursery_committed", g.nurseryCommitted);
    size_t code = zt.code.ion + zt.code.baseline + zt.code.regexp + zt.code.other + zt.code.unused;
    field("jit_code", code);
    field("jit_code_unused", zt.code.unused);
    j += "}";

    // the page's realm and zone, in Firefox's names (what a window's js-realm and js-zone hold)
    const JS::RealmStats* pr = nullptr;
    const JS::ZoneStats* pz = nullptr;
    for (auto& r : stats.realmStatsVector)
      if (r.extra == realm) pr = &r;
    for (auto& z : stats.zoneStatsVector)
      if (z.extra == zone) pz = &z;
    j += ",\"page\":{";
    if (pr) {
      JS::ServoSizes objects;
      pr->classInfo.addToServoSizes(&objects);
      for (auto& c : pr->notableClasses) c.addToServoSizes(&objects);
      field("objects", objects.gcHeapUsed + objects.mallocHeap + objects.nonHeap);
      field("scripts_gc_heap", pr->scriptsGCHeap + pr->scriptsGCBuffers);
      field("baseline_data", pr->baselineData);
      field("ion_data", pr->ionData);
      field("jit_scripts", pr->jitScripts);
      field("alloc_sites", pr->allocSites);
      field("realm_tables", pr->realmObject + pr->realmTables + pr->innerViewsTable + pr->objectMetadataTable +
                                pr->savedStacksSet + pr->nonSyntacticLexicalScopesTable);
    }
    if (pz) {
      size_t strings = pz->stringInfo.gcHeapLatin1 + pz->stringInfo.gcHeapTwoByte + pz->stringInfo.mallocHeapLatin1 +
                       pz->stringInfo.mallocHeapTwoByte;
      for (auto& s : pz->notableStrings)
        strings += s.gcHeapLatin1 + s.gcHeapTwoByte + s.mallocHeapLatin1 + s.mallocHeapTwoByte;
      field("strings", strings);
      const auto& sh = pz->shapeInfo;
      field("shapes", sh.shapesGCHeapShared + sh.shapesGCHeapDict + sh.shapesGCHeapBase + sh.shapesMallocHeapCache +
                          pz->shapeTables + pz->initialPropMapTable);
      field("prop_maps", pz->compactPropMapsGCHeap + pz->normalPropMapsGCHeap + pz->dictPropMapsGCHeap +
                             pz->propMapChildren + pz->propMapTables);
      field("scopes", pz->scopesGCHeap + pz->scopesGCBuffers);
      field("jit_zone", pz->jitZone + pz->cacheIRStubs + pz->jitCodesGCHeap);
      field("regexp", pz->regExpSharedsGCHeap + pz->regExpSharedsMallocHeap + pz->regexpZone);
      field("unused_gc_things", pz->unusedGCThings.totalSize());
      field("arena_admin", pz->gcHeapArenaAdmin);
      field("unique_id_map", pz->uniqueIdMap);
      field("buffers_free_admin", pz->gcBuffers.freeBytes + pz->gcBuffers.adminBytes);
    }
    j += "}}";
  }
  if (cap) {
    size_t n = j.size() < cap - 1 ? j.size() : cap - 1;
    memcpy(out, j.data(), n);
    out[n] = 0;
  }
  return j.size();
}

void mrt_js_memory(MrtJs* js, MrtJsMemory* out) {
  memset(out, 0, sizeof(*out));
  Stats stats;
  if (!JS::CollectRuntimeStats(js->cx, &stats, nullptr, false)) return;
  // The page's realm and zone are the ones of its global; the others are the runtime's own (the
  // atoms zone), which Firefox reports outside the window too.
  JS::Realm* realm = JS::GetObjectRealmOrNull(*js->global);
  JS::Zone* zone = JS::GetObjectZone(*js->global);
  JS::ServoSizes rs, zs;
  for (auto& r : stats.realmStatsVector) {
    if (getenv("MRT_JS_STATS")) {
      JS::ServoSizes t;
      r.addToServoSizes(&t);
      fprintf(stderr, "realm %p (page %p): used %zu malloc %zu\n", r.extra, (void*)realm, t.gcHeapUsed, t.mallocHeap);
    }
    if (r.extra == realm) {
      r.addToServoSizes(&rs);
      // the classes big enough to be "notable" were moved out of classInfo
      for (auto& c : r.notableClasses) c.addToServoSizes(&rs);
      if (getenv("MRT_JS_STATS")) {
        JS::ServoSizes other, all;
        r.classInfo.addToServoSizes(&other);
        r.addToServoSizes(&all);
        auto total = [](const JS::ServoSizes& t) { return t.gcHeapUsed + t.gcHeapUnused + t.gcHeapAdmin + t.mallocHeap + t.nonHeap; };
        fprintf(stderr, "realm detail: other classes %zu, non-class (scripts, tables) %zu\n", total(other), total(all) - total(other));
        for (auto& c : r.notableClasses) {
          JS::ServoSizes t;
          c.addToServoSizes(&t);
          fprintf(stderr, "  class %s: %zu (gc %zu)\n", c.className_.get(), total(t), t.gcHeapUsed);
        }
      }
    }
  }
  for (auto& z : stats.zoneStatsVector) {
    if (getenv("MRT_JS_STATS")) {
      JS::ServoSizes t;
      z.addToServoSizes(&t);
      fprintf(stderr, "zone %p (page %p): used %zu unused %zu malloc %zu; buffers used %zu free %zu admin %zu; jitZone %zu regexpZone %zu zoneObject %zu uniqueIdMap %zu shapeTables %zu cacheIR %zu\n", z.extra, (void*)zone, t.gcHeapUsed, t.gcHeapUnused, t.mallocHeap, z.gcBuffers.usedBytes, z.gcBuffers.freeBytes, z.gcBuffers.adminBytes, z.jitZone, z.regexpZone, z.zoneObject, z.uniqueIdMap, z.shapeTables, z.cacheIRStubs);
    }
    if (z.extra == zone) {
      z.addToServoSizes(&zs);
      for (auto& str : z.notableStrings) str.addToServoSizes(&zs);
      // Firefox's ReportZoneStats reports every field but the zone's GC buffer allocator: the bytes
      // in use are already in the realm, as each object's slots and elements (objects/gc-buffers),
      // and the free and admin bytes are not put under the window. Taken out to match.
      zs.mallocHeap -= z.gcBuffers.usedBytes + z.gcBuffers.freeBytes + z.gcBuffers.adminBytes;
      out->zone_buffer_slack = z.gcBuffers.freeBytes + z.gcBuffers.adminBytes;
      if (getenv("MRT_JS_STATS")) {
        // the page's zone under the names Firefox's ReportZoneStats uses, in bytes
        const auto& u = z.unusedGCThings;
        const auto& c = z.code;
        fprintf(stderr,
                "zone detail: unused-gc-things object %zu script %zu shape %zu baseShape %zu getterSetter %zu propMap %zu "
                "string %zu symbol %zu jitcode %zu scope %zu regExpShared %zu\n"
                "  code ion %zu baseline %zu regexp %zu other %zu unused %zu; jit-codes-gc-heap %zu\n"
                "  arena-admin %zu; strings gc %zu malloc %zu; shapes gc %zu %zu %zu malloc %zu; prop maps gc %zu %zu %zu "
                "malloc %zu %zu; getter-setters %zu; scopes %zu; regexp-shareds %zu %zu\n"
                "  malloc: zoneObject %zu regexpZone %zu jitZone %zu cacheIR %zu objectFuses %zu uniqueIdMap %zu "
                "initialPropMapTable %zu shapeTables %zu compartmentObjects %zu ccwTables %zu compartmentsPrivate %zu "
                "scriptCounts %zu\n",
                u.object, u.script, u.shape, u.baseShape, u.getterSetter, u.propMap, u.string, u.symbol, u.jitcode,
                u.scope, u.regExpShared, c.ion, c.baseline, c.regexp, c.other, c.unused, z.jitCodesGCHeap,
                z.gcHeapArenaAdmin, z.stringInfo.gcHeapLatin1 + z.stringInfo.gcHeapTwoByte,
                z.stringInfo.mallocHeapLatin1 + z.stringInfo.mallocHeapTwoByte, z.shapeInfo.shapesGCHeapShared,
                z.shapeInfo.shapesGCHeapDict, z.shapeInfo.shapesGCHeapBase, z.shapeInfo.shapesMallocHeapCache,
                z.compactPropMapsGCHeap, z.normalPropMapsGCHeap, z.dictPropMapsGCHeap, z.propMapChildren,
                z.propMapTables, z.getterSettersGCHeap, z.scopesGCHeap, z.regExpSharedsGCHeap,
                z.regExpSharedsMallocHeap, z.zoneObject, z.regexpZone, z.jitZone, z.cacheIRStubs, z.objectFuses,
                z.uniqueIdMap, z.initialPropMapTable, z.shapeTables, z.compartmentObjects,
                z.crossCompartmentWrappersTables, z.compartmentsPrivateData, z.scriptCountsMap);
      }
    }
  }
  if (getenv("MRT_JS_STATS")) {
    JS::ServoSizes t;
    stats.realmTotals.addToServoSizes(&t);
    stats.zTotals.addToServoSizes(&t);
    fprintf(stderr, "totals: used %zu malloc %zu; chunk total %zu\n", t.gcHeapUsed, t.mallocHeap, stats.gcHeapChunkTotal);
  }
  ToSizes(rs, &out->realm);
  ToSizes(zs, &out->zone);
  out->wrapper_cache = js->cache.capacity() * sizeof(JS::Heap<JSObject*>) + js->preserved.capacity();
  for (size_t i = 0; i < js->cache.size(); i++) {
    if (js->cache[i].unbarrieredGet()) {
      out->reflectors++;
      out->preserved += js->preserved[i];
    }
  }
}
