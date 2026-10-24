// Firefox picks a DOM class per element name through factory functions. This runtime has no such
// factories (an element is just its name), so the table entries are null.
#pragma once
// Marks an element name as a custom element (Firefox compares the creator function with NS_NewCustomElement).
#define MRT_CUSTOM_ELEMENT ((void*)1)
namespace mozilla::dom {
using HTMLContentCreatorFunction = void*;
using SVGContentCreatorFunction = void*;
}  // namespace mozilla::dom
union nsHtml5ContentCreatorFunction {
  void* html;
  void* svg;
};
