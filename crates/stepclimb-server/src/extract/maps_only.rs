// SPDX-License-Identifier: AGPL-3.0-or-later
//! A deserializer wrapper that reads every struct from a JSON object only.
//!
//! serde's derived `Deserialize` for a struct accepts a map and a sequence, and serde_json
//! hands a JSON array to the sequence form, so `[true, false]` would fill a two-field struct
//! in declaration order. [`MapsOnly`] wraps the deserializer, and every visitor, seed and
//! access it hands out, so the rule holds at every depth: `deserialize_struct` becomes
//! `deserialize_map`, which serde_json answers with an error for an array, and a struct
//! variant's visitor refuses a sequence. Everything else, real arrays (`Vec`, tuples)
//! included, passes through unchanged.
//!
//! The wrapper cannot reach a value serde buffers and replays with its own deserializer:
//! fields of a `#[serde(flatten)]` struct, the content of an untagged or internally tagged
//! enum, and the content of an adjacently tagged enum when it comes before the tag.
//! `tests/body_shapes.rs` fails on any `Deserialize` type of this crate that uses them. Nor
//! does it reach a value parsed later from a `serde_json::Value` the body carried: the
//! settings `PATCH` (`organization.rs`, `parse_patch`) takes a map and reads it into
//! `OrgSettingsPatch` with `serde_json::from_value`, whose fields are all scalars, and the
//! passkey routes read their free-form `credential` the same way.

use std::fmt;

use serde::de::{
    self, DeserializeSeed, Deserializer, EnumAccess, Error as _, MapAccess, SeqAccess, Unexpected,
    VariantAccess, Visitor,
};

/// `T` read with every struct, at any depth, taken from an object only.
pub struct MapsOnly<T>(pub T);

impl<'de, T: de::Deserialize<'de>> de::Deserialize<'de> for MapsOnly<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        T::deserialize(Wrap(deserializer)).map(Self)
    }
}

/// The wrapper around a deserializer, visitor, seed or access; each impl wraps what it hands
/// on.
struct Wrap<X>(X);

/// A struct variant's visitor: a sequence is refused.
struct StructVisitor<V>(V);

macro_rules! forward_deserialize {
    ($($method:ident)*) => {$(
        fn $method<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
            self.0.$method(Wrap(visitor))
        }
    )*};
}

impl<'de, D: Deserializer<'de>> Deserializer<'de> for Wrap<D> {
    type Error = D::Error;

    forward_deserialize! {
        deserialize_any deserialize_bool
        deserialize_i8 deserialize_i16 deserialize_i32 deserialize_i64 deserialize_i128
        deserialize_u8 deserialize_u16 deserialize_u32 deserialize_u64 deserialize_u128
        deserialize_f32 deserialize_f64 deserialize_char deserialize_str deserialize_string
        deserialize_bytes deserialize_byte_buf deserialize_option deserialize_unit
        deserialize_seq deserialize_map deserialize_identifier deserialize_ignored_any
    }

    fn deserialize_unit_struct<V: Visitor<'de>>(
        self,
        name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        self.0.deserialize_unit_struct(name, Wrap(visitor))
    }

    fn deserialize_newtype_struct<V: Visitor<'de>>(
        self,
        name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        self.0.deserialize_newtype_struct(name, Wrap(visitor))
    }

    fn deserialize_tuple<V: Visitor<'de>>(
        self,
        len: usize,
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        self.0.deserialize_tuple(len, Wrap(visitor))
    }

    fn deserialize_tuple_struct<V: Visitor<'de>>(
        self,
        name: &'static str,
        len: usize,
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        self.0.deserialize_tuple_struct(name, len, Wrap(visitor))
    }

    /// The point of the wrapper: a struct is read as a map, never as a sequence.
    fn deserialize_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        _fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        self.0.deserialize_map(Wrap(visitor))
    }

    fn deserialize_enum<V: Visitor<'de>>(
        self,
        name: &'static str,
        variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        self.0.deserialize_enum(name, variants, Wrap(visitor))
    }

    fn is_human_readable(&self) -> bool {
        self.0.is_human_readable()
    }
}

macro_rules! forward_visit {
    ($($method:ident($ty:ty))*) => {$(
        fn $method<E: de::Error>(self, value: $ty) -> Result<Self::Value, E> {
            self.0.$method(value)
        }
    )*};
}

impl<'de, V: Visitor<'de>> Visitor<'de> for Wrap<V> {
    type Value = V::Value;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.expecting(formatter)
    }

    forward_visit! {
        visit_bool(bool)
        visit_i8(i8) visit_i16(i16) visit_i32(i32) visit_i64(i64) visit_i128(i128)
        visit_u8(u8) visit_u16(u16) visit_u32(u32) visit_u64(u64) visit_u128(u128)
        visit_f32(f32) visit_f64(f64) visit_char(char)
        visit_str(&str) visit_borrowed_str(&'de str) visit_string(String)
        visit_bytes(&[u8]) visit_borrowed_bytes(&'de [u8]) visit_byte_buf(Vec<u8>)
    }

    fn visit_none<E: de::Error>(self) -> Result<Self::Value, E> {
        self.0.visit_none()
    }

    fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
        self.0.visit_unit()
    }

    fn visit_some<D: Deserializer<'de>>(self, deserializer: D) -> Result<Self::Value, D::Error> {
        self.0.visit_some(Wrap(deserializer))
    }

    fn visit_newtype_struct<D: Deserializer<'de>>(
        self,
        deserializer: D,
    ) -> Result<Self::Value, D::Error> {
        self.0.visit_newtype_struct(Wrap(deserializer))
    }

    fn visit_seq<A: SeqAccess<'de>>(self, seq: A) -> Result<Self::Value, A::Error> {
        self.0.visit_seq(Wrap(seq))
    }

    fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<Self::Value, A::Error> {
        self.0.visit_map(Wrap(map))
    }

    fn visit_enum<A: EnumAccess<'de>>(self, data: A) -> Result<Self::Value, A::Error> {
        self.0.visit_enum(Wrap(data))
    }
}

impl<'de, S: DeserializeSeed<'de>> DeserializeSeed<'de> for Wrap<S> {
    type Value = S::Value;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<S::Value, D::Error> {
        self.0.deserialize(Wrap(deserializer))
    }
}

impl<'de, A: SeqAccess<'de>> SeqAccess<'de> for Wrap<A> {
    type Error = A::Error;

    fn next_element_seed<T: DeserializeSeed<'de>>(
        &mut self,
        seed: T,
    ) -> Result<Option<T::Value>, A::Error> {
        self.0.next_element_seed(Wrap(seed))
    }

    fn size_hint(&self) -> Option<usize> {
        self.0.size_hint()
    }
}

impl<'de, A: MapAccess<'de>> MapAccess<'de> for Wrap<A> {
    type Error = A::Error;

    fn next_key_seed<K: DeserializeSeed<'de>>(
        &mut self,
        seed: K,
    ) -> Result<Option<K::Value>, A::Error> {
        self.0.next_key_seed(Wrap(seed))
    }

    fn next_value_seed<T: DeserializeSeed<'de>>(&mut self, seed: T) -> Result<T::Value, A::Error> {
        self.0.next_value_seed(Wrap(seed))
    }

    fn size_hint(&self) -> Option<usize> {
        self.0.size_hint()
    }
}

impl<'de, A: EnumAccess<'de>> EnumAccess<'de> for Wrap<A> {
    type Error = A::Error;
    type Variant = Wrap<A::Variant>;

    fn variant_seed<T: DeserializeSeed<'de>>(
        self,
        seed: T,
    ) -> Result<(T::Value, Self::Variant), A::Error> {
        let (value, variant) = self.0.variant_seed(Wrap(seed))?;
        Ok((value, Wrap(variant)))
    }
}

impl<'de, A: VariantAccess<'de>> VariantAccess<'de> for Wrap<A> {
    type Error = A::Error;

    fn unit_variant(self) -> Result<(), A::Error> {
        self.0.unit_variant()
    }

    fn newtype_variant_seed<T: DeserializeSeed<'de>>(self, seed: T) -> Result<T::Value, A::Error> {
        self.0.newtype_variant_seed(Wrap(seed))
    }

    fn tuple_variant<V: Visitor<'de>>(self, len: usize, visitor: V) -> Result<V::Value, A::Error> {
        self.0.tuple_variant(len, Wrap(visitor))
    }

    fn struct_variant<V: Visitor<'de>>(
        self,
        fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, A::Error> {
        self.0.struct_variant(fields, StructVisitor(Wrap(visitor)))
    }
}

impl<'de, V: Visitor<'de>> Visitor<'de> for StructVisitor<V> {
    type Value = V::Value;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.expecting(formatter)
    }

    fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<Self::Value, A::Error> {
        self.0.visit_map(map)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, _seq: A) -> Result<Self::Value, A::Error> {
        Err(A::Error::invalid_type(Unexpected::Seq, &self))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde::Deserialize;

    use super::MapsOnly;

    #[derive(Debug, Deserialize, PartialEq)]
    #[serde(deny_unknown_fields)]
    struct Settings {
        signup_allowed: bool,
        sync_only: bool,
    }

    #[derive(Debug, Deserialize, PartialEq)]
    struct Scope {
        #[serde(rename = "type")]
        kind: Kind,
        id: u32,
    }

    #[derive(Debug, Deserialize, PartialEq)]
    #[serde(rename_all = "lowercase")]
    enum Kind {
        Organization,
        Account,
    }

    #[derive(Debug, Deserialize, PartialEq)]
    struct Assignment {
        user: u32,
        scope: Scope,
        permissions: Vec<String>,
        note: Option<String>,
        reach: Option<Scope>,
        pair: (u8, u8),
    }

    #[derive(Debug, Deserialize, PartialEq)]
    #[serde(rename_all = "lowercase")]
    enum Change {
        Clear,
        Rename(String),
        Move { parent: u32, position: u32 },
    }

    #[derive(Debug, Deserialize, PartialEq)]
    struct Wrapper(Settings);

    fn parse<T: for<'de> Deserialize<'de>>(json: &str) -> Result<T, serde_json::Error> {
        serde_json::from_str::<MapsOnly<T>>(json).map(|parsed| parsed.0)
    }

    #[test]
    fn a_top_level_array_is_refused_for_a_struct() {
        assert!(serde_json::from_str::<Settings>("[true,false]").is_ok());
        let error = parse::<Settings>("[true,false]").expect_err("an array for a struct");
        assert!(
            error.to_string().contains("invalid type: sequence"),
            "{error}"
        );
    }

    #[test]
    fn a_nested_array_is_refused_for_a_struct() {
        let json = r#"{"user":1,"scope":["account",7],"permissions":[],"note":null,
            "reach":null,"pair":[1,2]}"#;
        assert!(serde_json::from_str::<Assignment>(json).is_ok());
        assert!(parse::<Assignment>(json).is_err());
    }

    #[test]
    fn a_struct_under_option_newtype_vec_and_map_is_refused_as_an_array() {
        let json = r#"{"user":1,"scope":{"type":"account","id":7},"permissions":[],
            "note":null,"reach":["account",7],"pair":[1,2]}"#;
        assert!(serde_json::from_str::<Assignment>(json).is_ok());
        assert!(parse::<Assignment>(json).is_err());
        assert!(serde_json::from_str::<Wrapper>("[true,false]").is_ok());
        assert!(parse::<Wrapper>("[true,false]").is_err());
        assert!(serde_json::from_str::<Vec<Settings>>("[[true,false]]").is_ok());
        assert!(parse::<Vec<Settings>>("[[true,false]]").is_err());
        let map = r#"{"a":[true,false]}"#;
        assert!(serde_json::from_str::<BTreeMap<String, Settings>>(map).is_ok());
        assert!(parse::<BTreeMap<String, Settings>>(map).is_err());
    }

    #[test]
    fn a_struct_variant_is_refused_as_an_array() {
        assert!(serde_json::from_str::<Change>(r#"{"move":[1,2]}"#).is_ok());
        assert!(parse::<Change>(r#"{"move":[1,2]}"#).is_err());
        assert_eq!(
            parse::<Change>(r#"{"move":{"parent":1,"position":2}}"#).expect("an object"),
            Change::Move {
                parent: 1,
                position: 2
            },
        );
    }

    #[test]
    fn objects_real_arrays_options_and_enums_still_parse() {
        let json = r#"{"user":1,"scope":{"type":"account","id":7},
            "permissions":["role.manage","user.manage"],"note":"hi",
            "reach":{"type":"organization","id":1},"pair":[3,4]}"#;
        assert_eq!(
            parse::<Assignment>(json).expect("an object"),
            Assignment {
                user: 1,
                scope: Scope {
                    kind: Kind::Account,
                    id: 7
                },
                permissions: vec!["role.manage".to_owned(), "user.manage".to_owned()],
                note: Some("hi".to_owned()),
                reach: Some(Scope {
                    kind: Kind::Organization,
                    id: 1
                }),
                pair: (3, 4),
            },
        );
        assert_eq!(parse::<Option<u8>>("null").expect("null"), None);
        assert_eq!(
            parse::<Change>(r#""clear""#).expect("a unit"),
            Change::Clear
        );
        assert_eq!(
            parse::<Change>(r#"{"rename":"x"}"#).expect("a newtype"),
            Change::Rename("x".to_owned()),
        );
        assert_eq!(
            parse::<Wrapper>(r#"{"signup_allowed":true,"sync_only":false}"#).expect("an object"),
            Wrapper(Settings {
                signup_allowed: true,
                sync_only: false
            }),
        );
        let value = parse::<serde_json::Value>(r#"[1,{"a":[true]}]"#).expect("any JSON");
        assert_eq!(value, serde_json::json!([1, {"a": [true]}]));
    }

    #[test]
    fn unknown_and_missing_fields_are_still_refused() {
        assert!(parse::<Settings>(r#"{"signup_allowed":true}"#).is_err());
        assert!(parse::<Settings>(r#"{"signup_allowed":true,"sync_only":true,"x":1}"#).is_err());
    }
}
