// Fixture: functions nested in `mod` blocks for qualified-name testing.
// Used by the complexity walker tests (crap-rs#283).
//
// The walker prepends the inline `mod` path to each function's name:
//   - top-level fn        → "top_level"
//   - fn in one mod        → "outer::in_outer"
//   - fn in nested mods    → "outer::inner::deep"
//   - method in a mod      → "outer::Widget::render" (mod path + Type::method)
//   - nested fn in a mod   → "outer::with_nested" (the inner `fn` is emitted
//                            file-scoped under the mod, NOT mod::outer::inner)

/// Top-level free function — qualified name is unchanged: "top_level".
pub fn top_level() {}

pub mod outer {
    /// One level deep — qualified: "outer::in_outer".
    pub fn in_outer() {}

    pub mod inner {
        /// Two levels deep — qualified: "outer::inner::deep".
        pub fn deep() {}
    }

    pub struct Widget;

    impl Widget {
        /// Method inside a mod — qualified: "outer::Widget::render"
        /// (mod path prepended to the impl-type qualification).
        pub fn render(&self) -> u32 {
            1
        }
    }

    /// A free fn in a mod that itself contains a nested `fn` item.
    /// The outer fn is "outer::with_nested"; the nested `helper` is
    /// emitted as "outer::helper" (mod-scoped, NOT fn-scoped) because
    /// the walker only threads `mod` nesting, not `fn` nesting.
    pub fn with_nested() {
        fn helper() {}
        helper();
    }
}
