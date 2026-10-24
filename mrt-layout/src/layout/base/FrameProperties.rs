// Gecko: layout/base/FrameProperties.h (class mozilla::FrameProperties), REDUCED
//
// The properties of one frame: a sparse list that is not allocated at all while the frame has no
// property, as Gecko's `nsTArray<PropertyValue> mProperties` is the 8-byte empty header of the
// array until the first Set. Gecko optimizes for 0 or 1 properties per frame, and so does this: a
// linear search over a short array.
//
// ADAPTER (what differs from the C++):
//  - The frame holds one pointer (8 bytes, null when empty) instead of an nsTArray, since a Rust Vec
//    is 24 bytes inline: to a node with the first property and an optional array of the others.
//  - Gecko's `FramePropertyDescriptor<T>` is the address of a static object; here (nsIFrame.rs) it is
//    a number, `mId`, which is also the key of an entry.
//  - Gecko's value is `uint64_t`: a pointer to a `new`-ed T for the "deletable" kinds, or the bits of a
//    small value (SmallValueHolder<T>). Here a value of up to 8 bytes with no destructor is stored
//    as its bits in the entry (as SmallValueHolder), and any other T in a Box, which is freed with
//    the entry (the deletable kinds: DeleteValue). `ReleaseValue`, `WITH_FRAME_IN_DTOR` and the other
//    destructor kinds are NOT PORTED: no property here needs them.
//  - Some deletable values are kept as a raw pointer (a leaked Box<T> stored as a usize) so that the
//    code can hand out a `&'static mut` to them, as Gecko's callers keep the `T*` it returns. Their
//    destructor is registered per property id (`RegisterOwnedPointer`) and runs where Gecko's
//    DeleteValue would: when the value is replaced by `Set` and in `RemoveAll` (nsIFrame::Destroy). Not
//    in `Remove`, whose callers free the value themselves first (GridPropRemove, the Destroy methods).
//  - `Get` returns a copy of the value (Gecko returns the pointer into the heap), as nsIFrame.rs says.
use std::any::Any;
use std::cell::RefCell;
use std::collections::HashMap;
use std::mem::{align_of, size_of};

thread_local! {
    // ADAPTER: the destructor of each raw-pointer property (see the header): property id to a function
    // that frees the Box<T> behind the pointer.
    static OWNED_POINTER_DELETERS: RefCell<HashMap<u32, unsafe fn(u64)>> = RefCell::new(HashMap::new());
}

unsafe fn DeleteOwnedPointer<T>(aValue: u64) {
    // SAFETY: the value came from Box::<T>::into_raw (RegisterOwnedPointer's contract)
    drop(unsafe { Box::from_raw(aValue as *mut T) });
}

// ADAPTER: from now on, a value of property `aProperty` is a pointer from `Box::<T>::into_raw`, owned by
// the frame (freed by `Set` over it and by `RemoveAll`). One type per property id.
pub fn RegisterOwnedPointer<T: 'static>(aProperty: u32) {
    OWNED_POINTER_DELETERS.with(|d| {
        d.borrow_mut().entry(aProperty).or_insert(DeleteOwnedPointer::<T>);
    });
}

// ADAPTER: runs the registered destructor of an entry, if its property has one.
fn DeleteIfOwnedPointer(aEntry: &FrameProperties_PropertyValue) {
    if aEntry.mIsBoxed {
        return;
    }
    if let Some(delete) = OWNED_POINTER_DELETERS.with(|d| d.borrow().get(&aEntry.mProperty).copied()) {
        unsafe { delete(aEntry.mValue) };
    }
}

// Gecko: layout/base/FrameProperties.h FrameProperties::PropertyValue
// (`const FramePropertyDescriptorUntyped* mProperty; uint64_t mValue;`)
pub struct FrameProperties_PropertyValue {
    // the id of the descriptor (Gecko: the descriptor's address)
    mProperty: u32,
    // whether mValue is a pointer to a boxed value (the "deletable" kind) instead of the value itself
    mIsBoxed: bool,
    mValue: u64,
}

impl Drop for FrameProperties_PropertyValue {
    // Gecko: FrameProperties::DestroyValueFor / RemoveAll (the descriptor's destructor, DeleteValue)
    fn drop(&mut self) {
        if self.mIsBoxed {
            // SAFETY: mValue came from Box::into_raw of a Box<Box<dyn Any>> in ToInternalValue
            drop(unsafe { Box::from_raw(self.mValue as *mut Box<dyn Any>) });
        }
    }
}

// Gecko: layout/base/FrameProperties.h FrameProperties::ReinterpretHelper::ToInternalValue
fn ToInternalValue<T: 'static>(aValue: T) -> (u64, bool) {
    if size_of::<T>() <= 8 && align_of::<T>() <= 8 && !std::mem::needs_drop::<T>() {
        let mut bits: u64 = 0;
        // SAFETY: T has no destructor and fits in 8 bytes; FromInternalValue reads the same bytes back
        unsafe {
            std::ptr::copy_nonoverlapping(&aValue as *const T as *const u8, &mut bits as *mut u64 as *mut u8, size_of::<T>());
        }
        std::mem::forget(aValue);
        (bits, false)
    } else {
        let boxed: Box<dyn Any> = Box::new(aValue);
        (Box::into_raw(Box::new(boxed)) as u64, true)
    }
}

impl FrameProperties_PropertyValue {
    // Gecko: layout/base/FrameProperties.h FrameProperties::ReinterpretHelper::FromInternalValue
    fn FromInternalValue<T: Clone + 'static>(&self) -> Option<T> {
        if self.mIsBoxed {
            // SAFETY: a boxed value is a live Box<Box<dyn Any>> until this entry is dropped
            let boxed: &Box<dyn Any> = unsafe { &*(self.mValue as *const Box<dyn Any>) };
            boxed.downcast_ref::<T>().cloned()
        } else if size_of::<T>() <= 8 && align_of::<T>() <= 8 && !std::mem::needs_drop::<T>() {
            let mut value = std::mem::MaybeUninit::<T>::uninit();
            // SAFETY: the bytes were copied from a T of this type by ToInternalValue (a descriptor id
            // has one type), and T has no destructor, so the copy is as good as the original
            unsafe {
                std::ptr::copy_nonoverlapping(&self.mValue as *const u64 as *const u8, value.as_mut_ptr() as *mut u8, size_of::<T>());
                Some(value.assume_init())
            }
        } else {
            None
        }
    }
}

// Gecko: layout/base/FrameProperties.h mozilla::FrameProperties
// ADAPTER: Gecko's `nsTArray<PropertyValue> mProperties`. The first property is in a node of its own
// (24 bytes, one small allocation) and the others, which most frames do not have, in an array hanging
// off that node. A frame with no property holds only the null pointer.
#[derive(Default)]
pub struct FrameProperties {
    mProperties: Option<Box<FrameProperties_Node>>,
}

// ADAPTER: see FrameProperties.
struct FrameProperties_Node {
    mFirst: FrameProperties_PropertyValue,
    mMore: Option<Box<Vec<FrameProperties_PropertyValue>>>,
}

impl FrameProperties {
    // Gecko: FrameProperties::IsEmpty
    // Return true if we have no properties, otherwise return false.
    pub fn IsEmpty(&self) -> bool {
        self.mProperties.is_none()
    }

    // The entry for aProperty, if there is one.
    fn Find(&self, aProperty: u32) -> Option<&FrameProperties_PropertyValue> {
        let node = self.mProperties.as_ref()?;
        if node.mFirst.mProperty == aProperty {
            return Some(&node.mFirst);
        }
        node.mMore.as_ref()?.iter().find(|e| e.mProperty == aProperty)
    }

    // Gecko: FrameProperties::Set
    // Set a property value. This requires a linear search through the properties of the frame. Any
    // existing value for the property is destroyed.
    pub fn Set<T: 'static>(&mut self, aProperty: u32, aValue: T) {
        let (value, boxed) = ToInternalValue(aValue);
        let entry = FrameProperties_PropertyValue { mProperty: aProperty, mIsBoxed: boxed, mValue: value };
        if let Some(node) = self.mProperties.as_mut() {
            if node.mFirst.mProperty == aProperty {
                // (assigning drops the old entry, which frees a boxed old value)
                DeleteIfOwnedPointer(&node.mFirst);
                node.mFirst = entry;
                return;
            }
            if let Some(old) = node.mMore.as_mut().and_then(|m| m.iter_mut().find(|e| e.mProperty == aProperty)) {
                DeleteIfOwnedPointer(old);
                *old = entry;
                return;
            }
        }
        self.AddInternal(entry);
    }

    // Gecko: FrameProperties::Has
    pub fn Has(&self, aProperty: u32) -> bool {
        self.Find(aProperty).is_some()
    }

    // Gecko: FrameProperties::Get
    // Get a property value (a copy of it). This requires a linear search through the properties of
    // the frame. None if the frame has no such property.
    pub fn Get<T: Clone + 'static>(&self, aProperty: u32) -> Option<T> {
        self.Find(aProperty)?.FromInternalValue::<T>()
    }

    // Gecko: FrameProperties::Remove
    // Remove a property value, destroying it. Returns true if the property was there.
    pub fn Remove(&mut self, aProperty: u32) -> bool {
        let Some(node) = self.mProperties.as_mut() else { return false };
        if node.mFirst.mProperty == aProperty {
            // the last of the others takes the first place, if there are any
            match node.mMore.as_mut().and_then(|m| m.pop()) {
                Some(last) => {
                    node.mFirst = last;
                    if node.mMore.as_ref().unwrap().is_empty() {
                        node.mMore = None;
                    }
                }
                None => self.mProperties = None,
            }
            return true;
        }
        let Some(more) = node.mMore.as_mut() else { return false };
        let Some(index) = more.iter().position(|e| e.mProperty == aProperty) else { return false };
        more.swap_remove(index);
        if more.is_empty() {
            // back to no array at all
            node.mMore = None;
        } else {
            more.shrink_to_fit();
        }
        true
    }

    // Gecko: FrameProperties::RemoveAll
    pub fn RemoveAll(&mut self) {
        if let Some(node) = self.mProperties.take() {
            DeleteIfOwnedPointer(&node.mFirst);
            for e in node.mMore.iter().flat_map(|m| m.iter()) {
                DeleteIfOwnedPointer(e);
            }
        }
    }

    // Gecko: FrameProperties::AddInternal
    // The array grows by exactly one entry: Gecko's nsTArray over-allocates, but with the 0 or 1
    // properties the frames have, that is the larger cost here.
    fn AddInternal(&mut self, aEntry: FrameProperties_PropertyValue) {
        match self.mProperties.as_mut() {
            None => self.mProperties = Some(Box::new(FrameProperties_Node { mFirst: aEntry, mMore: None })),
            Some(node) => {
                let more = node.mMore.get_or_insert_with(|| Box::new(Vec::new()));
                more.reserve_exact(1);
                more.push(aEntry);
            }
        }
    }
}
