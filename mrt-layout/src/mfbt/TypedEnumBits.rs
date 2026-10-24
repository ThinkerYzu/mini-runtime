// Gecko: mfbt/TypedEnumBits.h (MOZ_MAKE_ENUM_CLASS_BITWISE_OPERATORS)
// A C++ `enum class E : uint32_t { A = 1, B = 2 }` with bitwise operators becomes a newtype with
// associated constants and the operators, so `E::A | E::B` and `(bool)(flags & E::A)` keep their shape:
//     MOZ_ENUM_CLASS_BITS! { pub struct E: u32 { NoFlags = 0, HasFloats = 1 << 0 } }
// `bool(x & E::A)` is `(x & E::A).to_bool()`.

#[macro_export]
macro_rules! MOZ_ENUM_CLASS_BITS {
    ($(#[$m:meta])* $vis:vis struct $name:ident : $repr:ty { $($variant:ident = $value:expr),* $(,)? }) => {
        $(#[$m])*
        #[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
        $vis struct $name(pub $repr);
        impl $name {
            $(pub const $variant: $name = $name($value);)*
            pub fn to_bool(self) -> bool { self.0 != 0 }
        }
        impl std::ops::BitOr for $name { type Output = $name; fn bitor(self, o: $name) -> $name { $name(self.0 | o.0) } }
        impl std::ops::BitAnd for $name { type Output = $name; fn bitand(self, o: $name) -> $name { $name(self.0 & o.0) } }
        impl std::ops::BitXor for $name { type Output = $name; fn bitxor(self, o: $name) -> $name { $name(self.0 ^ o.0) } }
        impl std::ops::Not for $name { type Output = $name; fn not(self) -> $name { $name(!self.0) } }
        impl std::ops::BitOrAssign for $name { fn bitor_assign(&mut self, o: $name) { self.0 |= o.0 } }
        impl std::ops::BitAndAssign for $name { fn bitand_assign(&mut self, o: $name) { self.0 &= o.0 } }
    };
}
