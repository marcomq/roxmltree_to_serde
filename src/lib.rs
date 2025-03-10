#![allow(clippy::items_after_test_module)]
#![allow(clippy::single_match)]
#![allow(clippy::single_char_pattern)]
#![allow(clippy::needless_borrow)]
#![allow(clippy::ptr_arg)]
//! # roxmltree_to_serde
//! Fast and flexible conversion from XML to JSON using [quick-xml](https://github.com/tafia/quick-xml)
//! and [serde](https://github.com/serde-rs/json). Inspired by [node2object](https://github.com/vorot93/node2object).
//!
//! This crate converts XML elements, attributes and text nodes directly into corresponding JSON structures.
//! Some common usage scenarios would be converting XML into JSON for loading into No-SQL databases
//! or sending it to the front end application.
//!
//! Because of the richness and flexibility of XML some conversion behavior is configurable:
//! - attribute name prefixes
//! - naming of text nodes
//! - number format conversion
//!
//! ## Usage example
//! ```
//! extern crate roxmltree_to_serde;
//! use roxmltree_to_serde::{xml_string_to_json, Config, NullValue};
//!
//! fn main() {
//!    let xml = r#"<a attr1="1"><b><c attr2="001">some text</c></b></a>"#;
//!    let conf = Config::new_with_defaults();
//!    let json = xml_string_to_json(xml.to_owned(), &conf);
//!    println!("{}", json.expect("Malformed XML").to_string());
//!
//!    let conf = Config::new_with_custom_values(true, "", "txt", NullValue::Null);
//!    let json = xml_string_to_json(xml.to_owned(), &conf);
//!    println!("{}", json.expect("Malformed XML").to_string());
//! }
//! ```
//! * **Output with the default config:** `{"a":{"@attr1":1,"b":{"c":{"#text":"some text","@attr2":1}}}}`
//! * **Output with a custom config:** `{"a":{"attr1":1,"b":{"c":{"attr2":"001","txt":"some text"}}}}`
//!
//! ## Additional features
//! Use `roxmltree_to_serde = { version = "0.4", features = ["json_types"] }` to enable support for enforcing JSON types
//! for some XML nodes using xPath-like notations. Example for enforcing attribute `attr2` from the snippet above
//! as JSON String regardless of its contents:
//! ```
//! use roxmltree_to_serde::{Config, JsonArray, JsonType};
//!
//! #[cfg(feature = "json_types")]
//! let conf = Config::new_with_defaults()
//!            .add_json_type_override("/a/b/c/@attr2", JsonArray::Infer(JsonType::AlwaysString));
//! ```
//!
//! ## Detailed documentation
//! See [README](https://github.com/marcomq/roxmltree_to_serde) in the source repo for more examples, limitations and detailed behavior description.
//!
//! ## Testing your XML files
//!
//! If you want to see how your XML files are converted into JSON, place them into `./test_xml_files` directory
//! and run `cargo test`. They will be converted into JSON and saved in the saved directory.

extern crate roxmltree;
extern crate serde_json;

#[cfg(feature = "regex_path")]
extern crate regex;

use serde_json::{Map, Number, Value};
#[cfg(feature = "json_types")]
use std::collections::HashMap;

#[cfg(feature = "regex_path")]
use regex::Regex;

#[cfg(test)]
mod tests;

/// Defines how empty elements like `<x />` should be handled.
/// `Ignore` -> exclude from JSON, `Null` -> `"x":null`, EmptyObject -> `"x":{}`.
/// `EmptyObject` is the default option and is how it was handled prior to v.0.4
/// Using `Ignore` on an XML document with an empty root element falls back to `Null` option.
/// E.g. both `<a><x/></a>` and `<a/>` are converted into `{"a":null}`.
#[derive(Debug)]
pub enum NullValue {
    Ignore,
    Null,
    EmptyObject,
}

/// Defines how the values of this Node should be converted into a JSON array with the underlying types.
/// * `Infer` - the nodes are converted into a JSON array only if there are multiple identical elements.
/// E.g. `<a><b>1</b></a>` becomes a map `{"a": {"b": 1 }}` and `<a><b>1</b><b>2</b><b>3</b></a>` becomes
/// an array `{"a": {"b": [1, 2, 3] }}`
/// * `Always` - the nodes are converted into a JSON array regardless of how many there are.
/// E.g. `<a><b>1</b></a>` becomes an array with a single value `{"a": {"b": [1] }}` and
/// `<a><b>1</b><b>2</b><b>3</b></a>` also becomes an array `{"a": {"b": [1, 2, 3] }}`
#[derive(Debug)]
pub enum JsonArray {
    /// Convert the nodes into a JSON array even if there is only one element
    Always(JsonType),
    /// Convert the nodes into a JSON array only if there are multiple identical elements
    Infer(JsonType),
}

/// Used as a parameter for `Config.add_json_type_override`. Defines how the XML path should be matched
/// in order to apply the JSON type overriding rules. This enumerator exists to allow the same function
/// to be used for multiple different types of path matching rules.
#[derive(Debug)]
pub enum PathMatcher {
    /// An absolute path starting with a leading slash (`/`). E.g. `/a/b/c/@d`.
    /// It's implicitly converted from `&str` and automatically includes the leading slash.
    Absolute(String),
    /// A regex that will be checked against the XML path. E.g. `(\w/)*c$`.
    /// It's implicitly converted from `regex::Regex`.
    #[cfg(feature = "regex_path")]
    Regex(Regex),
}

// For retro-compatibility and for syntax's sake, a string may be coerced into an absolute path.
impl From<&str> for PathMatcher {
    fn from(value: &str) -> Self {
        let path_with_leading_slash = if value.starts_with("/") {
            value.into()
        } else {
            ["/", value].concat()
        };

        PathMatcher::Absolute(path_with_leading_slash)
    }
}

// ... While a Regex may be coerced into a regex path.
#[cfg(feature = "regex_path")]
impl From<Regex> for PathMatcher {
    fn from(value: Regex) -> Self {
        PathMatcher::Regex(value)
    }
}

/// Defines which data type to apply in JSON format for consistency of output.
/// E.g., the range of XML values for the same node type may be `1234`, `001234`, `AB1234`.
/// It is impossible to guess with 100% consistency which data type to apply without seeing
/// the entire range of values. Use this enum to tell the converter which data type should
/// be applied.
#[derive(Debug, PartialEq, Clone)]
pub enum JsonType {
    /// Do not try to infer the type and convert the value to JSON string.
    /// E.g. convert `<a>1234</a>` into `{"a":"1234"}` or `<a>true</a>` into `{"a":"true"}`
    AlwaysString,
    /// Convert values included in this member into JSON bool `true` and any other value into `false`.
    /// E.g. `Bool(vec!["True", "true", "TRUE"]) will result in any of these values to become JSON bool `true`.
    Bool(Vec<&'static str>),
    /// Attempt to infer the type by looking at the single value of the node being converted.
    /// Not guaranteed to be consistent across multiple nodes.
    /// E.g. convert `<a>1234</a>` and `<a>001234</a>` into `{"a":1234}`, or `<a>true</a>` into `{"a":true}`
    /// Check if your values comply with JSON data types (case, range, format) to produce the expected result.
    Infer,
}

/// Tells the converter how to perform certain conversions.
/// See docs for individual fields for more info.
#[derive(Debug)]
pub struct Config {
    /// Numeric values starting with 0 will be treated as strings.
    /// E.g. convert `<agent>007</agent>` into `"agent":"007"` or `"agent":7`
    /// Defaults to `false`.
    pub leading_zero_as_string: bool,
    /// Prefix XML attribute names with this value to distinguish them from XML elements.
    /// E.g. set it to `@` for `<x a="Hello!" />` to become `{"x": {"@a":"Hello!"}}`
    /// or set it to a blank string for `{"x": {"a":"Hello!"}}`
    /// Defaults to `@`.
    pub xml_attr_prefix: String,
    /// A property name for XML text nodes.
    /// E.g. set it to `text` for `<x a="Hello!">Goodbye!</x>` to become `{"x": {"@a":"Hello!", "text":"Goodbye!"}}`
    /// XML nodes with text only and no attributes or no child elements are converted into JSON properties with the
    /// name of the element. E.g. `<x>Goodbye!</x>` becomes `{"x":"Goodbye!"}`
    /// Defaults to `#text`
    pub xml_text_node_prop_name: String,
    /// Defines how empty elements like `<x />` should be handled.
    pub empty_element_handling: NullValue,
    /// A map of XML paths with their JsonArray overrides. They take precedence over the document-wide `json_type`
    /// property. The path syntax is based on xPath: literal element names and attribute names prefixed with `@`.
    /// The path must start with a leading `/`. It is a bit of an inconvenience to remember about it, but it saves
    /// an extra `if`-check in the code to improve the performance.
    /// # Example
    /// - **XML**: `<a><b c="123">007</b></a>`
    /// - path for `c`: `/a/b/@c`
    /// - path for `b` text node (007): `/a/b`
    #[cfg(feature = "json_types")]
    pub json_type_overrides: HashMap<String, JsonArray>,
    /// A list of pairs of regex and JsonArray overrides. They take precedence over both the document-wide `json_type`
    /// property and the `json_type_overrides` property. The path syntax is based on xPath just like `json_type_overrides`.
    #[cfg(feature = "regex_path")]
    pub json_regex_type_overrides: Vec<(Regex, JsonArray)>,
}

impl Config {
    /// Numbers with leading zero will be treated as numbers.
    /// Prefix XML Attribute names with `@`
    /// Name XML text nodes `#text` for XML Elements with other children
    pub fn new_with_defaults() -> Self {
        Config {
            leading_zero_as_string: false,
            xml_attr_prefix: "@".to_owned(),
            xml_text_node_prop_name: "#text".to_owned(),
            empty_element_handling: NullValue::EmptyObject,
            #[cfg(feature = "json_types")]
            json_type_overrides: HashMap::new(),
            #[cfg(feature = "regex_path")]
            json_regex_type_overrides: Vec::new(),
        }
    }

    /// Create a Config object with non-default values. See the `Config` struct docs for more info.
    pub fn new_with_custom_values(
        leading_zero_as_string: bool,
        xml_attr_prefix: &str,
        xml_text_node_prop_name: &str,
        empty_element_handling: NullValue,
    ) -> Self {
        Config {
            leading_zero_as_string,
            xml_attr_prefix: xml_attr_prefix.to_owned(),
            xml_text_node_prop_name: xml_text_node_prop_name.to_owned(),
            empty_element_handling,
            #[cfg(feature = "json_types")]
            json_type_overrides: HashMap::new(),
            #[cfg(feature = "regex_path")]
            json_regex_type_overrides: Vec::new(),
        }
    }

    /// Adds a single JSON Type override rule to the current config.
    /// # Example
    /// - **XML**: `<a><b c="123">007</b></a>`
    /// - path for `c`: `/a/b/@c`
    /// - path for `b` text node (007): `/a/b`
    /// - regex path for any `element` node: `(\w/)*element$` [requires `regex_path` feature]
    #[cfg(feature = "json_types")]
    pub fn add_json_type_override<P>(self, path: P, json_type: JsonArray) -> Self
    where
        P: Into<PathMatcher>,
    {
        let mut conf = self;

        match path.into() {
            PathMatcher::Absolute(path) => {
                conf.json_type_overrides.insert(path, json_type);
            }
            #[cfg(feature = "regex_path")]
            PathMatcher::Regex(regex) => {
                conf.json_regex_type_overrides.push((regex, json_type));
            }
        }

        conf
    }
}

impl Default for Config {
    fn default() -> Self {
        Config::new_with_defaults()
    }
}

/// Returns the text as one of `serde::Value` types: int, float, bool or string.
fn parse_text(text: &str, leading_zero_as_string: bool, json_type: &JsonType) -> Value {
    let text = text.trim();

    // enforce JSON String data type regardless of the underlying type
    if json_type == &JsonType::AlwaysString {
        return Value::String(text.into());
    }

    // enforce JSON Bool data type
    #[cfg(feature = "json_types")]
    if let JsonType::Bool(true_values) = json_type {
        if true_values.contains(&text) {
            // any values matching the `true` list are bool/true
            return Value::Bool(true);
        } else {
            // anything else is false
            return Value::Bool(false);
        }
    }

    // ints
    if let Ok(v) = text.parse::<u64>() {
        // don't parse octal numbers and those with leading 0
        // `text` value "0" will always be converted into number 0, "0000" may be converted
        // into 0 or "0000" depending on `leading_zero_as_string`
        if leading_zero_as_string && text.starts_with("0") && (v != 0 || text.len() > 1) {
            return Value::String(text.into());
        }
        return Value::Number(Number::from(v));
    }

    // floats
    if let Ok(v) = text.parse::<f64>() {
        if text.starts_with("0") && !text.starts_with("0.") {
            return Value::String(text.into());
        }
        if let Some(val) = Number::from_f64(v) {
            return Value::Number(val);
        }
    }

    // booleans
    if let Ok(v) = text.parse::<bool>() {
        return Value::Bool(v);
    }

    Value::String(text.into())
}

fn convert_text(
    el: &roxmltree::Node,
    config: &Config,
    text: &str,
    json_type_value: JsonType,
) -> Option<Value> {
    // process node's attributes, if present
    if el.attributes().count() > 0 {
        Some(Value::Object(
            el.attributes()
                .map(|attr| {
                    // add the current node to the path
                    #[cfg(feature = "json_types")]
                    let path = [path.clone(), "/@".to_owned(), attr.name().to_string()].concat();
                    // get the json_type for this node
                    #[cfg(feature = "json_types")]
                    let (_, json_type_value) = get_json_type(config, &path);
                    (
                        [config.xml_attr_prefix.clone(), attr.name().to_string()].concat(),
                        parse_text(
                            attr.value(),
                            config.leading_zero_as_string,
                            &json_type_value,
                        ),
                    )
                })
                .chain(vec![(
                    config.xml_text_node_prop_name.clone(),
                    parse_text(&text[..], config.leading_zero_as_string, &json_type_value),
                )])
                .collect(),
        ))
    } else {
        Some(parse_text(
            &text[..],
            config.leading_zero_as_string,
            &json_type_value,
        ))
    }
}

fn convert_no_text(
    el: &roxmltree::Node,
    config: &Config,
    path: &String,
    json_type_value: JsonType,
) -> Option<Value> {
    // this element has no text, but may have other child nodes
    let mut data = Map::new();

    for attr in el.attributes() {
        // add the current node to the path
        #[cfg(feature = "json_types")]
        let path = [path.clone(), "/@".to_owned(), attr.name().to_string()].concat();
        // get the json_type for this node
        #[cfg(feature = "json_types")]
        let (_, json_type_value) = get_json_type(config, &path);
        data.insert(
            [config.xml_attr_prefix.clone(), attr.name().to_string()].concat(),
            parse_text(
                attr.value(),
                config.leading_zero_as_string,
                &json_type_value,
            ),
        );
    }

    // process child element recursively
    for child in el.children() {
        match convert_node(&child, config, &path) {
            Some(val) => {
                let name = &child.tag_name().name().to_string();
                if name == "" {
                    ()
                } else {
                    #[cfg(feature = "json_types")]
                    let path = [path.clone(), "/".to_owned(), name.clone()].concat();
                    let (json_type_array, _) = get_json_type(config, &path);

                    // does it have to be an array?
                    if json_type_array || data.contains_key(name) {
                        // was this property converted to an array earlier?
                        if data.get(name).unwrap_or(&Value::Null).is_array() {
                            // add the new value to an existing array
                            data.get_mut(name)
                                .unwrap()
                                .as_array_mut()
                                .unwrap()
                                .push(val);
                        } else {
                            // convert the property to an array with the existing and the new values
                            let new_val = match data.remove(name) {
                                None => vec![val],
                                Some(temp) => vec![temp, val],
                            };
                            data.insert(name.clone(), Value::Array(new_val));
                        }
                    } else {
                        // this is the first time this property is encountered and it doesn't
                        // have to be an array, so add it as-is
                        data.insert(name.clone(), val);
                    }
                }
            }
            _ => (),
        }
    }

    // return the JSON object if it's not empty
    if !data.is_empty() {
        return Some(Value::Object(data));
    }

    // empty objects are treated according to config rules set by the caller
    match config.empty_element_handling {
        NullValue::Null => Some(Value::Null),
        NullValue::EmptyObject => Some(Value::Object(data)),
        NullValue::Ignore => None,
    }
}

/// Converts an XML Element into a JSON property
fn convert_node(el: &roxmltree::Node, config: &Config, path: &String) -> Option<Value> {
    // add the current node to the path
    #[cfg(feature = "json_types")]
    let path = [path, "/", el.tag_name().name()].concat();

    // get the json_type for this node
    let (_, json_type_value) = get_json_type(config, &path);
    let json_type_value = json_type_value.clone();

    // is it an element with text?
    match el.text() {
        Some(mut text) => {
            text = text.trim();

            if text != "" {
                convert_text(el, config, text, json_type_value)
            } else {
                convert_no_text(el, config, path, json_type_value)
            }
        }
        None => convert_no_text(el, config, path, json_type_value),
    }
}

fn xml_to_map(e: &roxmltree::Node, config: &Config) -> Value {
    let mut data = Map::new();
    data.insert(
        e.tag_name().name().to_string(),
        convert_node(&e, &config, &String::new()).unwrap_or(Value::Null),
    );
    Value::Object(data)
}

/// Converts the given XML string into `serde::Value` using settings from `Config` struct.
pub fn xml_str_to_json(xml: &str, config: &Config) -> Result<Value, roxmltree::Error> {
    let doc = roxmltree::Document::parse(xml)?;
    let root = doc.root_element();
    Ok(xml_to_map(&root, config))
}

/// Converts the given XML string into `serde::Value` using settings from `Config` struct.
pub fn xml_string_to_json(xml: String, config: &Config) -> Result<Value, roxmltree::Error> {
    xml_str_to_json(xml.as_str(), config)
}

/// Returns a tuple for Array and Value enforcements for the current node or
/// `(false, JsonArray::Infer(JsonType::Infer)` if the current path is not found
/// in the list of paths with custom config.
#[cfg(feature = "json_types")]
#[inline]
fn get_json_type_with_absolute_path<'conf>(
    config: &'conf Config,
    path: &String,
) -> (bool, &'conf JsonType) {
    match config
        .json_type_overrides
        .get(path)
        .unwrap_or(&JsonArray::Infer(JsonType::Infer))
    {
        JsonArray::Infer(v) => (false, v),
        JsonArray::Always(v) => (true, v),
    }
}

/// Simply returns `get_json_type_with_absolute_path` if `regex_path` feature is disabled.
#[cfg(feature = "json_types")]
#[cfg(not(feature = "regex_path"))]
#[inline]
fn get_json_type<'conf>(config: &'conf Config, path: &String) -> (bool, &'conf JsonType) {
    get_json_type_with_absolute_path(config, path)
}

/// Returns a tuple for Array and Value enforcements for the current node. Searches both absolute paths
/// and regex paths, giving precedence to regex paths. Returns `(false, JsonArray::Infer(JsonType::Infer)`
/// if the current path is not found in the list of paths with custom config.
#[cfg(feature = "json_types")]
#[cfg(feature = "regex_path")]
#[inline]
fn get_json_type<'conf>(config: &'conf Config, path: &String) -> (bool, &'conf JsonType) {
    for (regex, json_array) in &config.json_regex_type_overrides {
        if regex.is_match(path) {
            return match json_array {
                JsonArray::Infer(v) => (false, v),
                JsonArray::Always(v) => (true, v),
            };
        }
    }

    get_json_type_with_absolute_path(config, path)
}

/// Always returns `(false, JsonArray::Infer(JsonType::Infer)` if `json_types` feature is not enabled.
#[cfg(not(feature = "json_types"))]
#[inline]
fn get_json_type<'conf>(_config: &'conf Config, _path: &String) -> (bool, &'conf JsonType) {
    (false, &JsonType::Infer)
}
