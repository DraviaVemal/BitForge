use std::env;
use std::path::PathBuf;

use log::error;

#[derive(Debug, Clone)]
pub struct AppArgs {
    pub working_directory: PathBuf,
    pub init_requested: bool,
    pub init_dir_name: Option<String>,
    pub poky_version: Option<String>,
    pub build_requested: bool,
    pub build_target: Option<String>,
    pub version_requested: bool,
    pub update_requested: bool,
    pub beta: bool,
    pub force: bool,
    pub dependency_add_spec: Option<String>,
    pub dependency_remove_name: Option<String>,
    pub add_layer_name: Option<String>,
    pub remove_layer_name: Option<String>,
}

impl AppArgs {
    pub fn parse() -> Self {
        let mut arguments: Vec<String> = env::args().collect();
        if !arguments.is_empty() {
            arguments.remove(0);
        }
        match Self::parse_from(arguments) {
            Ok(args) => args,
            Err(message) => {
                error!("{message}");
                std::process::exit(2);
            }
        }
    }

    fn parse_from(arguments: Vec<String>) -> Result<Self, String> {
        let working_directory = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

        let mut init_requested = false;
        let mut init_dir_name: Option<String> = None;
        let mut poky_version: Option<String> = None;
        let mut build_requested = false;
        let mut build_target: Option<String> = None;
        let mut version_requested = false;
        let mut update_requested = false;
        let mut beta = false;
        let mut force = false;
        let mut dependency_add_spec: Option<String> = None;
        let mut dependency_remove_name: Option<String> = None;
        let mut add_layer_name: Option<String> = None;
        let mut remove_layer_name: Option<String> = None;
        let mut unknown: Vec<String> = Vec::new();

        let mut index = 0;
        while index < arguments.len() {
            let Some(flag) = arguments[index].strip_prefix("--") else {
                let raw = arguments[index].clone();
                if raw != "." && raw != "./" {
                    unknown.push(raw);
                }
                index += 1;
                continue;
            };
            let (flag_name, inline_value) = split_flag(flag);

            match flag_name.as_str() {
                "init" => {
                    init_requested = true;
                    init_dir_name = value_for(inline_value, &arguments, &mut index);
                }
                "build" => {
                    build_requested = true;
                    build_target = value_for(inline_value, &arguments, &mut index);
                }
                "poky" => {
                    poky_version =
                        Some(value_for(inline_value, &arguments, &mut index).unwrap_or_default());
                }
                "force" => force = true,
                "version" => version_requested = true,
                "update" => update_requested = true,
                "beta" => beta = true,
                "dependency-add" => {
                    dependency_add_spec = value_for(inline_value, &arguments, &mut index)
                }
                "dependency-remove" => {
                    dependency_remove_name = value_for(inline_value, &arguments, &mut index)
                }
                "add-layer" => add_layer_name = value_for(inline_value, &arguments, &mut index),
                "remove-layer" => {
                    remove_layer_name = value_for(inline_value, &arguments, &mut index)
                }
                other => unknown.push(format!("--{other}")),
            }

            index += 1;
        }

        if !unknown.is_empty() {
            return Err(format!("unknown argument(s): {}", unknown.join(", ")));
        }

        Ok(Self {
            working_directory,
            init_requested,
            init_dir_name,
            poky_version,
            build_requested,
            build_target,
            version_requested,
            update_requested,
            beta,
            force,
            dependency_add_spec,
            dependency_remove_name,
            add_layer_name,
            remove_layer_name,
        })
    }
}

fn split_flag(flag: &str) -> (String, Option<String>) {
    match flag.split_once('=') {
        Some((name, value)) => (name.to_string(), Some(unquote(value))),
        None => (flag.to_string(), None),
    }
}

fn value_for(
    inline_value: Option<String>,
    arguments: &[String],
    index: &mut usize,
) -> Option<String> {
    if inline_value.is_some() {
        return inline_value;
    }
    optional_value(arguments, index)
}

fn optional_value(arguments: &[String], index: &mut usize) -> Option<String> {
    match arguments.get(*index + 1) {
        Some(next) if !next.starts_with('-') => {
            *index += 1;
            Some(unquote(next))
        }
        _ => None,
    }
}

fn unquote(value: &str) -> String {
    let characters: Vec<char> = value.chars().collect();
    if characters.len() >= 2 {
        let first = characters[0];
        let last = characters[characters.len() - 1];
        if (first == '"' && last == '"') || (first == '\'' && last == '\'') {
            return characters[1..characters.len() - 1].iter().collect();
        }
    }
    value.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_equals_form_and_unquotes() {
        assert_eq!(split_flag("poky=scarthgap"), ("poky".into(), Some("scarthgap".into())));
        assert_eq!(split_flag("init=\"demo poky\""), ("init".into(), Some("demo poky".into())));
        assert_eq!(split_flag("force"), ("force".into(), None));
    }

    #[test]
    fn space_form_consumes_next_argument() {
        let arguments = vec!["--init".to_string(), "demo poky".to_string()];
        let mut index = 0;
        assert_eq!(optional_value(&arguments, &mut index), Some("demo poky".into()));
        assert_eq!(index, 1);
    }

    #[test]
    fn value_for_prefers_inline_value() {
        let arguments = vec!["--poky".to_string(), "next".to_string()];
        let mut index = 0;
        assert_eq!(
            value_for(Some("scarthgap".into()), &arguments, &mut index),
            Some("scarthgap".into())
        );
        assert_eq!(index, 0);
    }

    #[test]
    fn unquote_strips_matching_quotes_only() {
        assert_eq!(unquote("\"a b\""), "a b");
        assert_eq!(unquote("'a b'"), "a b");
        assert_eq!(unquote("plain"), "plain");
        assert_eq!(unquote("\"unterminated"), "\"unterminated");
    }

    #[test]
    fn unknown_flag_errors() {
        let error = AppArgs::parse_from(vec!["--iniy".into(), "demo".into()]).unwrap_err();
        assert!(error.contains("--iniy"), "{error}");
    }

    #[test]
    fn stray_positional_errors() {
        let error = AppArgs::parse_from(vec!["stray".into()]).unwrap_err();
        assert!(error.contains("stray"), "{error}");
    }

    #[test]
    fn current_dir_marker_and_known_flags_are_accepted() {
        let args = AppArgs::parse_from(vec![".".into(), "--init".into(), "demo".into()]).unwrap();
        assert!(args.init_requested);
        assert_eq!(args.init_dir_name.as_deref(), Some("demo"));
    }
}
