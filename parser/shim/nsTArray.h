// The slice of Firefox's nsTArray / AutoTArray that the parser core uses, on top of std::vector.
#pragma once
#include <cstddef>
#include <cstdint>
#include <utility>
#include <vector>

template <class T>
class nsTArray : public std::vector<T> {
 public:
  uint32_t Length() const { return (uint32_t)this->size(); }
  bool IsEmpty() const { return this->empty(); }
  T* Elements() { return this->data(); }
  const T* Elements() const { return this->data(); }
  T& ElementAt(size_t i) { return (*this)[i]; }
  template <class... Args>
  T* AppendElement(Args&&... args) {
    this->emplace_back(std::forward<Args>(args)...);
    return &this->back();
  }
  void SetCapacity(size_t n) { this->reserve(n); }
  void Clear() { this->clear(); }
  void ClearAndRetainStorage() { this->clear(); }
};

template <class T, size_t N>
class AutoTArray : public nsTArray<T> {};
