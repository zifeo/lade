use std::collections::HashMap;

use ini::Ini;
use serde_json::Value;

pub fn ini2json(ini: Ini) -> Value {
    let mut map = HashMap::new();
    for (section, properties) in ini.iter() {
        let mut section_map = HashMap::new();
        for (key, value) in properties.iter() {
            section_map.insert(key.to_string(), Value::String(value.to_string()));
        }
        match section {
            Some(section) => {
                map.insert(
                    section.to_string(),
                    Value::Object(section_map.into_iter().collect()),
                );
            }
            None => map.extend(section_map),
        }
    }
    Value::Object(map.into_iter().collect())
}

pub fn toml2json(toml: toml::Value) -> Value {
    match toml {
        toml::Value::String(s) => Value::String(s),
        toml::Value::Integer(i) => Value::Number(i.into()),
        toml::Value::Float(f) => {
            Value::Number(serde_json::Number::from_f64(f).expect("nan not allowed"))
        }
        toml::Value::Boolean(b) => Value::Bool(b),
        toml::Value::Array(arr) => Value::Array(arr.into_iter().map(toml2json).collect()),
        toml::Value::Table(table) => {
            Value::Object(table.into_iter().map(|(k, v)| (k, toml2json(v))).collect())
        }
        toml::Value::Datetime(dt) => Value::String(dt.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_toml2json_string() {
        assert_eq!(
            toml2json(toml::Value::String("hello".to_string())),
            Value::String("hello".to_string())
        );
    }

    #[test]
    fn test_toml2json_integer() {
        assert_eq!(
            toml2json(toml::Value::Integer(42)),
            Value::Number(42.into())
        );
    }

    #[test]
    fn test_toml2json_boolean() {
        assert_eq!(toml2json(toml::Value::Boolean(true)), Value::Bool(true));
    }

    #[test]
    fn test_toml2json_array() {
        let arr = toml::Value::Array(vec![
            toml::Value::String("a".into()),
            toml::Value::String("b".into()),
        ]);
        assert_eq!(
            toml2json(arr),
            Value::Array(vec![Value::String("a".into()), Value::String("b".into())])
        );
    }

    #[test]
    fn test_toml2json_nested_table() {
        let mut inner = toml::map::Map::new();
        inner.insert(
            "nested_key".to_string(),
            toml::Value::String("nested_val".to_string()),
        );
        let mut outer = toml::map::Map::new();
        outer.insert("section".to_string(), toml::Value::Table(inner));
        let result = toml2json(toml::Value::Table(outer));
        if let Value::Object(map) = result {
            if let Some(Value::Object(section)) = map.get("section") {
                assert_eq!(
                    section.get("nested_key").unwrap(),
                    &Value::String("nested_val".to_string())
                );
            } else {
                panic!("expected section Object");
            }
        } else {
            panic!("expected Object");
        }
    }

    #[test]
    fn test_ini2json_section_with_properties() {
        let ini = Ini::load_from_str("[section1]\nkey1 = val1\n").unwrap();
        let result = ini2json(ini);
        if let Value::Object(map) = result {
            if let Some(Value::Object(section)) = map.get("section1") {
                assert_eq!(
                    section.get("key1").unwrap(),
                    &Value::String("val1".to_string())
                );
            } else {
                panic!("expected section1 Object");
            }
        } else {
            panic!("expected Object");
        }
    }

    #[test]
    fn test_ini2json_global_key() {
        let ini = Ini::load_from_str("global_key = global_val\n").unwrap();
        let result = ini2json(ini);
        if let Value::Object(map) = result {
            assert_eq!(
                map.get("global_key").unwrap(),
                &Value::String("global_val".to_string())
            );
        } else {
            panic!("expected Object");
        }
    }

    #[test]
    fn test_ini2json_multiple_sections() {
        let ini = Ini::load_from_str("[s1]\nk1 = v1\n\n[s2]\nk2 = v2\n").unwrap();
        let result = ini2json(ini);
        if let Value::Object(map) = result {
            assert!(map.contains_key("s1"));
            assert!(map.contains_key("s2"));
        } else {
            panic!("expected Object");
        }
    }
}
