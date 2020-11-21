use std::collections::HashMap;

use serde_json::{Value, Map};

fn insert_with_path(object: &mut Value, path: &[&str], key_pos: usize, value: &str) {
    match object.as_object_mut() {
        Some(m) => {
            if key_pos == (path.len() - 1) {
                // Simple key
                m.insert(path[key_pos].to_owned(), Value::String(value.to_owned()));
            }
            else {
                match m.get_mut(path[key_pos]) {
                    Some(next) => insert_with_path(next, path, key_pos + 1, value),
                    None => {
                        // New object at this path
                        let new_obj_name = path[key_pos];
                        m.insert(new_obj_name.to_owned(), Value::Object(Map::new()));
                        insert_with_path(m.get_mut(new_obj_name).unwrap(), path, key_pos + 1, value);
                    }
                }
            }
        },
        None => eprintln!("WARNING: Key {} ignored because the dotted prefix is already in use", path.join("."))
    }
}

pub fn build_template_model(data: HashMap<String, String>) -> Value {
    let mut sorted_keys: Vec<String> = data.keys().cloned().collect();
    sorted_keys.sort(); // Maybe should have been a BTreeMap?

    let mut result = Value::Object(Map::new());

    for key in &sorted_keys {
        let parts: Vec<&str> = key.split('.').collect();
        let value = data.get(key).unwrap();
        insert_with_path(&mut result, &parts, 0, value);
    }

    result
}
