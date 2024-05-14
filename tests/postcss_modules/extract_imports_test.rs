use std::borrow::Cow;

use css_module_lexer::Dependency;
use css_module_lexer::LexDependencies;
use css_module_lexer::Lexer;
use css_module_lexer::Mode;
use css_module_lexer::ModeData;
use css_module_lexer::Range;
use css_module_lexer::Warning;
use indoc::indoc;
use linked_hash_map::LinkedHashMap;

#[derive(Debug, Default, Clone, Hash, PartialEq, Eq)]
pub struct ExtractImports;

impl ExtractImports {
    pub fn transform<'s>(&self, input: &'s str) -> (String, Vec<Warning<'s>>) {
        let mut imported = String::new();
        let mut result = String::new();
        let mut warnings = Vec::new();
        let mut index = 0;
        let mut lexer = Lexer::new(input);
        let mut composes_contents = Vec::new();
        let mut postfix = 0;
        let mut imported_values: LinkedHashMap<&str, LinkedHashMap<&str, Cow<str>>> =
            LinkedHashMap::new();
        let mut visitor = LexDependencies::new(
            |dependency| match dependency {
                Dependency::Composes { names, from } => {
                    let names: Vec<_> = names.split(' ').collect();
                    let mut composes_content = String::new();
                    if let Some(from) = from {
                        if from == "global" {
                            for i in 0..names.len() {
                                let name = names[i];
                                composes_content += "global(";
                                composes_content += name;
                                composes_content += ")";
                                if i + 1 != names.len() {
                                    composes_content += " ";
                                }
                            }
                        } else {
                            let path = from.trim_matches(|c| c == '\'' || c == '"');
                            let values = imported_values.entry(path).or_default();
                            for i in 0..names.len() {
                                let name = names[i];
                                if let Some(value) = values.get(name) {
                                    composes_content += &value;
                                } else {
                                    let value = format!("i__imported_{name}_{postfix}");
                                    postfix += 1;
                                    composes_content += &value;
                                    values.insert(name, value.into());
                                }
                                if i + 1 != names.len() {
                                    composes_content += " ";
                                }
                            }
                        }
                    } else {
                        for i in 0..names.len() {
                            let name = names[i];
                            composes_content += name;
                            if i + 1 != names.len() {
                                composes_content += " ";
                            }
                        }
                    }
                    composes_contents.push(composes_content);
                }
                Dependency::Replace { content, range } => {
                    if !composes_contents.is_empty() {
                        let composes_contents = std::mem::take(&mut composes_contents);
                        result +=
                            Lexer::slice_range(input, &Range::new(index, range.start)).unwrap();
                        result += "composes: ";
                        result += &composes_contents.join(", ");
                        result += ";";
                        index = range.end;
                    } else {
                        let original = Lexer::slice_range(input, &range).unwrap();
                        if original.starts_with(":export") || original.starts_with(":import(") {
                            result +=
                                Lexer::slice_range(input, &Range::new(index, range.start)).unwrap();
                            result += content;
                            index = range.end;
                        }
                    }
                }
                Dependency::ICSSImportFrom { path } => {
                    let path = path.trim_matches(|c| c == '\'' || c == '"');
                    imported_values.insert(path, LinkedHashMap::new());
                }
                Dependency::ICSSImportValue { prop, value } => {
                    let (_, values) = imported_values.iter_mut().last().unwrap();
                    values.insert(value, prop.into());
                }
                _ => {}
            },
            |warning| warnings.push(warning),
            Some(ModeData::new(Mode::Local)),
        );
        lexer.lex(&mut visitor);
        let len = input.len() as u32;
        if index != len {
            result += Lexer::slice_range(input, &Range::new(index, len)).unwrap();
        }
        for (path, values) in imported_values {
            imported += ":import(\"";
            imported += path;
            imported += "\") {\n";
            for (value, prop) in values {
                imported += "    ";
                imported += &prop;
                imported += ": ";
                imported += value;
                imported += ";\n";
            }
            imported += "}\n";
        }
        (imported + &result, warnings)
    }
}

fn test(input: &str, expected: &str) {
    let (actual, warnings) = ExtractImports::default().transform(input);
    assert_eq!(expected, actual);
    assert!(warnings.is_empty(), "{}", &warnings[0]);
}

#[test]
fn composing_globals() {
    test(
        ":local(.exportName) { composes: importName secondImport from global; other: rule; }",
        ":local(.exportName) { composes: global(importName) global(secondImport); other: rule; }",
    );
}

#[test]
fn existing_import() {
    test(
        indoc! {r#"
            :import("path/library.css") {
                something: else;
            }
            :local(.exportName) {
                composes: importName from 'path/library.css';
            }
        "#},
        indoc! {r#"
            :import("path/library.css") {
                something: else;
                i__imported_importName_0: importName;
            }

            :local(.exportName) {
                composes: i__imported_importName_0;
            }
        "#},
    );
}

#[test]
fn import_comment() {
    test(
        indoc! {r#"
            /*
            :local(.exportName) {
                composes: importName from "path/library.css";
                other: rule;
            }
            */
        "#},
        indoc! {r#"
            /*
            :local(.exportName) {
                composes: importName from "path/library.css";
                other: rule;
            }
            */
        "#},
    );
}

#[test]
fn import_consolidate() {
    test(
        indoc! {r#"
            :local(.exportName) {
                composes: importName secondImport from 'path/library.css';
                other: rule;
            }
            :local(.otherExport) {
                composes: thirdImport from 'path/library.css';
                composes: otherLibImport from 'path/other-lib.css';
            }
        "#},
        indoc! {r#"
            :import("path/library.css") {
                i__imported_importName_0: importName;
                i__imported_secondImport_1: secondImport;
                i__imported_thirdImport_2: thirdImport;
            }
            :import("path/other-lib.css") {
                i__imported_otherLibImport_3: otherLibImport;
            }
            :local(.exportName) {
                composes: i__imported_importName_0 i__imported_secondImport_1;
                other: rule;
            }
            :local(.otherExport) {
                composes: i__imported_thirdImport_2;
                composes: i__imported_otherLibImport_3;
            }
        "#},
    );
}

#[test]
fn import_local_extends() {
    test(
        indoc! {r#"
            :local(.exportName) {
                composes: localName;
                other: rule;
            }
        "#},
        indoc! {r#"
            :local(.exportName) {
                composes: localName;
                other: rule;
            }
        "#},
    );
}

#[test]
fn import_media() {
    test(
        indoc! {r#"
            @media screen {
                :local(.exportName) {
                    composes: importName from "path/library.css";
                    composes: importName2 from "path/library.css";
                    other: rule2;
                }
            }
            
            :local(.exportName) {
                composes: importName from "path/library.css";
                other: rule;
            }
        "#},
        indoc! {r#"
            :import("path/library.css") {
                i__imported_importName_0: importName;
                i__imported_importName2_1: importName2;
            }
            @media screen {
                :local(.exportName) {
                    composes: i__imported_importName_0;
                    composes: i__imported_importName2_1;
                    other: rule2;
                }
            }

            :local(.exportName) {
                composes: i__imported_importName_0;
                other: rule;
            }
        "#},
    );
}

#[test]
fn import_multiple_classes() {
    test(
        ":local(.exportName) { composes: importName secondImport from 'path/library.css'; other: rule; }\n",
        indoc! {r#"
            :import("path/library.css") {
                i__imported_importName_0: importName;
                i__imported_secondImport_1: secondImport;
            }
            :local(.exportName) { composes: i__imported_importName_0 i__imported_secondImport_1; other: rule; }
        "#},
    );
}
