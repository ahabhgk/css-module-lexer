use crate::Dependency;
use crate::LexDependencies;
use crate::Lexer;
use crate::Mode;
use crate::ModeData;
use crate::Range;
use crate::Warning;

#[derive(Debug, Default, Clone, Hash, PartialEq, Eq)]
pub struct LocalByDefault {
    pub mode: Mode,
}

impl LocalByDefault {
    pub fn transform<'s>(&self, input: &'s str) -> (String, Vec<Warning<'s>>) {
        let mut result = String::new();
        let mut warnings = Vec::new();
        let mut index = 0;
        let mut lexer = Lexer::new(input);
        let mut visitor = LexDependencies::new(
            |dependency| match dependency {
                Dependency::LocalIdent { name, range }
                | Dependency::LocalKeyframesDecl { name, range }
                | Dependency::LocalKeyframes { name, range } => {
                    result += Lexer::slice_range(input, &Range::new(index, range.start)).unwrap();
                    result += ":local(";
                    result += name;
                    result += ")";
                    index = range.end;
                }
                Dependency::Replace { content, range } => {
                    if Lexer::slice_range(input, &range)
                        .unwrap()
                        .starts_with(":export")
                    {
                        return;
                    }
                    result += Lexer::slice_range(input, &Range::new(index, range.start)).unwrap();
                    result += content;
                    index = range.end;
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
