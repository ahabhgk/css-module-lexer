use std::collections::HashSet;

use crate::Dependency;
use crate::LexDependencies;
use crate::Lexer;
use crate::Mode;
use crate::ModeData;
use crate::Pos;
use crate::Range;
use crate::Warning;

#[derive(Debug, Default, Clone, Hash, PartialEq, Eq)]
pub struct LocalByDefault {
    pub mode: Mode,
}

fn add_local(result: &mut String, input: &str, name: &str, start: Pos, end: Pos) {
    *result += Lexer::slice_range(input, &Range::new(start, end)).unwrap();
    *result += ":local(";
    *result += name;
    *result += ")";
}

impl LocalByDefault {
    pub fn transform<'s>(&self, input: &'s str) -> (String, Vec<Warning<'s>>) {
        let mut result = String::new();
        let mut warnings = Vec::new();
        let mut index = 0;
        let mut lexer = Lexer::new(input);
        let mut local_alias = HashSet::new();
        let mut visitor = LexDependencies::new(
            |dependency| match dependency {
                Dependency::LocalIdent {
                    name,
                    range,
                    explicit,
                } => {
                    if let Some(name) = name.strip_prefix('.') {
                        if !explicit && local_alias.contains(name) {
                            return;
                        }
                    }
                    add_local(&mut result, input, name, index, range.start);
                    index = range.end;
                }
                Dependency::LocalKeyframes { name, range } => {
                    if local_alias.contains(name) {
                        return;
                    }
                    add_local(&mut result, input, name, index, range.start);
                    index = range.end;
                }
                Dependency::LocalKeyframesDecl { name, range } => {
                    add_local(&mut result, input, name, index, range.start);
                    index = range.end;
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
                Dependency::ICSSImportValue { prop, .. } => {
                    local_alias.insert(prop);
                }
                _ => {}
            },
            |warning| warnings.push(warning),
            Some(ModeData::new(self.mode)),
        );
        lexer.lex(&mut visitor);
        let len = input.len() as u32;
        if index != len {
            result += Lexer::slice_range(input, &Range::new(index, len)).unwrap();
        }
        (result, warnings)
    }
}
