//! crate 内部宏。

/// 开放字符串联合（TS `'a' | 'b' | (string & {})`）的 Rust 等价：
/// `#[serde(transparent)]` 的 String 新类型 + 已知值关联常量。
///
/// 保留类型识别（区别于裸 `String`）、round-trip 任意未知值（bug-for-bug，
/// 不拒绝上游新增值），并暴露 `as_str()` / `Display` / `From`。
macro_rules! open_string_union {
    ($(#[$meta:meta])* $name:ident { $($(#[$cmeta:meta])* $cname:ident => $val:literal),* $(,)? }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq, Hash, ::serde::Serialize, ::serde::Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub String);

        impl $name {
            $( $(#[$cmeta])* pub const $cname: &'static str = $val; )*

            /// 借出底层 wire 字符串。
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl From<String> for $name {
            fn from(s: String) -> Self {
                Self(s)
            }
        }
        impl From<&str> for $name {
            fn from(s: &str) -> Self {
                Self(s.to_string())
            }
        }
        impl ::std::fmt::Display for $name {
            fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}

pub(crate) use open_string_union;
