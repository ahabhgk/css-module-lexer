use css_module_lexer::Dependency;
use css_module_lexer::LexDependencies;
use css_module_lexer::Lexer;
use css_module_lexer::Mode;
use css_module_lexer::ModeData;
use css_module_lexer::Pos;
use css_module_lexer::Range;
use css_module_lexer::Warning;
use indoc::indoc;
use linked_hash_map::LinkedHashMap;

#[derive(Debug, Default, Clone, Hash, PartialEq, Eq)]
struct Scope;

fn generate_local_name(name: &str) -> String {
    format!("_input__{}", name)
}

impl Scope {
    pub fn transform<'s>(&self, input: &'s str) -> (String, Vec<Warning<'s>>) {
        let mut result = String::new();
        let mut warnings = Vec::new();
        let mut index = 0;
        let mut lexer = Lexer::new(input);
        let mut exports = LinkedHashMap::new();
        // This is not correct, only for passing tests
        let mut last_local_class = None;
        let rename_local = |result: &mut String,
                            exports: &mut LinkedHashMap<String, Vec<String>>,
                            name: &str,
                            start: Pos,
                            end: Pos| {
            *result += Lexer::slice_range(input, &Range::new(start, end)).unwrap();
            let is_class = name.starts_with('.');
            let is_id = name.starts_with('#');
            let name = if is_class || is_id { &name[1..] } else { name };
            if is_class {
                *result += ".";
            } else if is_id {
                *result += "#";
            }
            let new_name = generate_local_name(name);
            *result += &new_name;
            exports.insert(name.to_string(), vec![new_name]);
        };
        let mut visitor = LexDependencies::new(
            |dependency| match dependency {
                Dependency::LocalIdent { name, range, .. } => {
                    if let Some(name) = name.strip_prefix('.') {
                        last_local_class = Some(name);
                    }
                    rename_local(&mut result, &mut exports, name, index, range.start);
                    index = range.end;
                }
                Dependency::LocalKeyframes { name, range }
                | Dependency::LocalKeyframesDecl { name, range } => {
                    rename_local(&mut result, &mut exports, name, index, range.start);
                    index = range.end;
                }
                Dependency::Composes { names, from } => {
                    let names: Vec<_> = names.split_whitespace().collect();
                    let local_class = last_local_class.unwrap();
                    for name in names {
                        let new_name = if matches!(from, Some("global")) {
                            name.to_string()
                        } else {
                            generate_local_name(name)
                        };
                        exports.get_mut(local_class).unwrap().push(new_name);
                    }
                }
                Dependency::Replace { content, range } => {
                    let original = Lexer::slice_range(input, &range).unwrap();
                    if original.starts_with(":export") || original.starts_with(":import(") {
                        return;
                    }
                    result += Lexer::slice_range(input, &Range::new(index, range.start)).unwrap();
                    result += content;
                    index = range.end;
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
        if !exports.is_empty() {
            result += "\n:export {\n";
            for (key, value) in exports {
                result += "    ";
                result += &key;
                result += ": ";
                result += &value.join(" ");
                result += ";\n";
            }
            result += "}\n";
        }
        (result, warnings)
    }
}

fn test(input: &str, expected: &str) {
    let (actual, warnings) = Scope::default().transform(input);
    assert!(warnings.is_empty(), "{}", &warnings[0]);
    similar_asserts::assert_eq!(expected, actual);
}

#[test]
fn at_rule() {
    test(
        indoc! {r#"
            :local(.otherClass) {
                background: red;
            }

            @media screen {
                :local(.foo) {
                    color: green;
                    :local(.baz) {
                        color: blue;
                    }
                }
            }
        "#},
        indoc! {r#"
            ._input__otherClass {
                background: red;
            }

            @media screen {
                ._input__foo {
                    color: green;
                    ._input__baz {
                        color: blue;
                    }
                }
            }

            :export {
                otherClass: _input__otherClass;
                foo: _input__foo;
                baz: _input__baz;
            }
        "#},
    );
}

#[test]
fn at_rule_scope() {
    test(
        indoc! {r#"
            :local(.d) {
                color: red;
            }
            
            @scope (:local(.a)) to (:local(.b)) {
                :local(.c) {
                    border: 5px solid black;
                    background-color: goldenrod;
                }
            }
            
            @scope (:local(.a)) {
                :local(.e) {
                    border: 5px solid black;
                }
            }
            
            @scope (:local(.a)) to (img) {
                :local(.f) {
                    background-color: goldenrod;
                }
            }
            
            @scope (:local(.g)) {
                img {
                    backdrop-filter: blur(2px);
                }
            }
            
            @scope {
                :scope {
                    color: red;
                }
            }
        "#},
        indoc! {r#"
            ._input__d {
                color: red;
            }
            
            @scope (._input__a) to (._input__b) {
                ._input__c {
                    border: 5px solid black;
                    background-color: goldenrod;
                }
            }
            
            @scope (._input__a) {
                ._input__e {
                    border: 5px solid black;
                }
            }
            
            @scope (._input__a) to (img) {
                ._input__f {
                    background-color: goldenrod;
                }
            }
            
            @scope (._input__g) {
                img {
                    backdrop-filter: blur(2px);
                }
            }
            
            @scope {
                :scope {
                    color: red;
                }
            }
            
            :export {
                d: _input__d;
                b: _input__b;
                c: _input__c;
                e: _input__e;
                a: _input__a;
                f: _input__f;
                g: _input__g;
            }
        "#},
    );
}

#[test]
fn composes_only_allowed() {
    test(
        indoc! {r#"
            :local(.class) {
                composes: global(a);
                compose-with: global(b);
                a-composes: global(c);
                composes-b: global(d);
                a-composes-b: global(e);
                a-compose-with-b: global(b);
            }
        "#},
        indoc! {r#"
            ._input__class {
                
                
                a-composes: global(c);
                composes-b: global(d);
                a-composes-b: global(e);
                a-compose-with-b: global(b);
            }

            :export {
                class: _input__class a b;
            }
        "#},
    );
}

#[test]
fn css_nesting() {
    test(
        indoc! {r#"
            :local(.otherClass) {
                background: red;
            }

            :local(.foo) {
                color: green;

                @media (max-width: 520px) {
                    :local(.bar) {
                        color: darkgreen;
                    }

                    &:local(.baz) {
                        color: blue;
                    }
                }
            }

            :local(.a) {
                color: red;

                &:local(.b) {
                    color: green;
                }

                :local(.c) {
                    color: blue;
                }
            }
        "#},
        indoc! {r#"
            ._input__otherClass {
                background: red;
            }

            ._input__foo {
                color: green;

                @media (max-width: 520px) {
                    ._input__bar {
                        color: darkgreen;
                    }

                    &._input__baz {
                        color: blue;
                    }
                }
            }

            ._input__a {
                color: red;

                &._input__b {
                    color: green;
                }

                ._input__c {
                    color: blue;
                }
            }

            :export {
                otherClass: _input__otherClass;
                foo: _input__foo;
                bar: _input__bar;
                baz: _input__baz;
                a: _input__a;
                b: _input__b;
                c: _input__c;
            }
        "#},
    );
}

#[test]
fn css_nesting_composes() {
    test(
        indoc! {r#"
            :local(.bar) {
                color: red;
            }

            :local(.foo) {
                display: grid;
                composes: bar;

                @media (orientation: landscape) {
                    grid-auto-flow: column;
                }
            }
        "#},
        indoc! {r#"
            ._input__bar {
                color: red;
            }

            ._input__foo {
                display: grid;
                

                @media (orientation: landscape) {
                    grid-auto-flow: column;
                }
            }

            :export {
                bar: _input__bar;
                foo: _input__foo _input__bar;
            }
        "#},
    );
}
