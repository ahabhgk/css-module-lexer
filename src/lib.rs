use std::str::CharIndices;

const C_LINE_FEED: char = '\n';
const C_CARRIAGE_RETURN: char = '\r';
const C_FORM_FEED: char = '\u{c}';

const C_TAB: char = '\t';
const C_SPACE: char = ' ';

const C_SOLIDUS: char = '/';
const C_REVERSE_SOLIDUS: char = '\\';
const C_ASTERISK: char = '*';

const C_LEFT_PARENTHESIS: char = '(';
const C_RIGHT_PARENTHESIS: char = ')';
const C_LEFT_CURLY: char = '{';
const C_RIGHT_CURLY: char = '}';
const C_LEFT_SQUARE: char = '[';
const C_RIGHT_SQUARE: char = ']';

const C_QUOTATION_MARK: char = '"';
const C_APOSTROPHE: char = '\'';

const C_FULL_STOP: char = '.';
const C_COLON: char = ':';
const C_SEMICOLON: char = ';';
const C_COMMA: char = ',';
const C_PERCENTAGE: char = '%';
const C_AT_SIGN: char = '@';

const C_LOW_LINE: char = '_';
const C_LOWER_A: char = 'a';
const C_LOWER_E: char = 'e';
const C_LOWER_F: char = 'f';
const C_LOWER_Z: char = 'z';
const C_UPPER_A: char = 'A';
const C_UPPER_E: char = 'E';
const C_UPPER_F: char = 'F';
const C_UPPER_Z: char = 'Z';
const C_0: char = '0';
const C_9: char = '9';

const C_NUMBER_SIGN: char = '#';
const C_PLUS_SIGN: char = '+';
const C_HYPHEN_MINUS: char = '-';

const C_LESS_THAN_SIGN: char = '<';
const C_GREATER_THAN_SIGN: char = '>';

pub trait Handler {
    fn function(&mut self, lexer: &mut Lexer, start: usize, end: usize) -> Option<()>;
    fn ident(&mut self, lexer: &mut Lexer, start: usize, end: usize) -> Option<()>;
    fn url(
        &mut self,
        lexer: &mut Lexer,
        start: usize,
        end: usize,
        content_start: usize,
        content_end: usize,
    ) -> Option<()>;
    fn string(&mut self, lexer: &mut Lexer, start: usize, end: usize) -> Option<()>;
    fn is_selector(&mut self, lexer: &mut Lexer) -> Option<bool>;
    fn id(&mut self, lexer: &mut Lexer, start: usize, end: usize) -> Option<()>;
    fn left_parenthesis(&mut self, lexer: &mut Lexer, start: usize, end: usize) -> Option<()>;
    fn right_parenthesis(&mut self, lexer: &mut Lexer, start: usize, end: usize) -> Option<()>;
    fn comma(&mut self, lexer: &mut Lexer, start: usize, end: usize) -> Option<()>;
    fn class(&mut self, lexer: &mut Lexer, start: usize, end: usize) -> Option<()>;
    fn pseudo_function(&mut self, lexer: &mut Lexer, start: usize, end: usize) -> Option<()>;
    fn pseudo_class(&mut self, lexer: &mut Lexer, start: usize, end: usize) -> Option<()>;
    fn semicolon(&mut self, lexer: &mut Lexer, start: usize, end: usize) -> Option<()>;
    fn at_keyword(&mut self, lexer: &mut Lexer, start: usize, end: usize) -> Option<()>;
    fn left_curly(&mut self, lexer: &mut Lexer, start: usize, end: usize) -> Option<()>;
    fn right_curly(&mut self, lexer: &mut Lexer, start: usize, end: usize) -> Option<()>;
}

pub struct Lexer<'s> {
    value: &'s str,
    iter: CharIndices<'s>,
    cur: Option<(usize, char)>,
    peek: Option<(usize, char)>,
    peek2: Option<(usize, char)>,
}

impl<'s> From<&'s str> for Lexer<'s> {
    fn from(value: &'s str) -> Self {
        let mut iter = value.char_indices();
        let peek = iter.next();
        let peek2 = iter.next();
        Self {
            value,
            iter,
            cur: None,
            peek,
            peek2,
        }
    }
}

impl Lexer<'_> {
    #[must_use]
    pub fn consume(&mut self) -> Option<char> {
        self.cur = self.peek;
        self.peek = self.peek2;
        self.peek2 = self.iter.next();
        self.cur()
    }

    pub fn cur_pos(&self) -> Option<usize> {
        self.cur.map(|(p, _)| p)
    }

    pub fn cur(&self) -> Option<char> {
        self.cur.map(|(_, c)| c)
    }

    pub fn peek_pos(&self) -> Option<usize> {
        self.peek.map(|(p, _)| p)
    }

    pub fn peek(&self) -> Option<char> {
        self.peek.map(|(_, c)| c)
    }

    pub fn peek2_pos(&self) -> Option<usize> {
        self.peek2.map(|(p, _)| p)
    }

    pub fn peek2(&self) -> Option<char> {
        self.peek2.map(|(_, c)| c)
    }

    pub fn slice(&self, start: usize, end: usize) -> Option<&str> {
        self.value.get(start..end)
    }

    pub fn is_eof(&self) -> bool {
        self.cur.is_none()
    }
}

impl Lexer<'_> {
    pub fn lex<T: Handler>(&mut self, handler: &mut T) {
        self.lex_impl(handler);
    }

    fn lex_impl<T: Handler>(&mut self, handler: &mut T) -> Option<()> {
        self.consume()?;
        while !self.is_eof() {
            self.consume_comments()?;
            let c = self.cur()?;
            if c < '\u{80}' {
                // https://drafts.csswg.org/css-syntax/#consume-token
                match c {
                    c if is_white_space(c) => self.consume_space()?,
                    C_QUOTATION_MARK => self.consume_string(handler, C_QUOTATION_MARK)?,
                    C_NUMBER_SIGN => self.consume_number_sign(handler)?,
                    C_APOSTROPHE => self.consume_string(handler, C_APOSTROPHE)?,
                    C_LEFT_PARENTHESIS => self.consume_left_parenthesis(handler)?,
                    C_RIGHT_PARENTHESIS => self.consume_right_parenthesis(handler)?,
                    C_PLUS_SIGN => self.consume_plus_sign()?,
                    C_COMMA => self.consume_comma(handler)?,
                    C_HYPHEN_MINUS => self.consume_minus(handler)?,
                    C_FULL_STOP => self.consume_full_stop(handler)?,
                    C_COLON => self.consume_potential_pseudo(handler)?,
                    C_SEMICOLON => self.consume_semicolon(handler)?,
                    C_LESS_THAN_SIGN => self.consume_less_than_sign()?,
                    C_AT_SIGN => self.consume_at_sign(handler)?,
                    C_LEFT_SQUARE => self.consume_delim()?,
                    C_REVERSE_SOLIDUS => self.consume_reverse_solidus(handler)?,
                    C_RIGHT_SQUARE => self.consume_delim()?,
                    C_LEFT_CURLY => self.consume_left_curly(handler)?,
                    C_RIGHT_CURLY => self.consume_right_curly(handler)?,
                    c if is_digit(c) => self.consume_numeric_token()?,
                    c if is_ident_start(c) => self.consume_ident_like(handler)?,
                    _ => self.consume_delim()?,
                }
            }
        }
        Some(())
    }

    fn consume_comments(&mut self) -> Option<()> {
        if self.cur()? == C_SOLIDUS && self.peek()? == C_ASTERISK {
            while let Some(c) = self.consume() {
                if c == C_ASTERISK && self.peek()? == C_SOLIDUS {
                    self.consume()?;
                    self.consume()?;
                    break;
                }
            }
        }
        Some(())
    }

    fn consume_delim(&mut self) -> Option<()> {
        self.consume()?;
        Some(())
    }

    fn consume_space(&mut self) -> Option<()> {
        while is_white_space(self.consume()?) {}
        Some(())
    }

    fn consume_numeric_token(&mut self) -> Option<()> {
        self.consume_number()?;
        let c = self.cur()?;
        if start_ident_sequence(c, self.peek()?, self.peek2()?) {
            return self.consume_ident_sequence();
        }
        if c == C_PERCENTAGE {
            self.consume()?;
        }
        Some(())
    }

    fn consume_number(&mut self) -> Option<()> {
        while matches!(self.consume(), Some(c) if is_digit(c)) {}
        if self.cur()? == C_FULL_STOP && is_digit(self.peek()?) {
            self.consume()?;
            while matches!(self.consume(), Some(c) if is_digit(c)) {}
        }
        let c = self.cur()?;
        if c == C_LOWER_E || c == C_UPPER_E {
            let c = self.peek()?;
            if is_digit(c) {
                self.consume()?;
            } else if c == C_HYPHEN_MINUS || c == C_PLUS_SIGN {
                let c = self.peek2()?;
                if is_digit(c) {
                    self.consume()?;
                    self.consume()?;
                } else {
                    return Some(());
                }
            } else {
                return Some(());
            }
        } else {
            return Some(());
        }
        while matches!(self.consume(), Some(c) if is_digit(c)) {}
        Some(())
    }

    fn consume_ident_sequence(&mut self) -> Option<()> {
        loop {
            let c = self.cur()?;
            if are_valid_escape(c, self.peek()?) {
                self.consume()?;
                self.consume_escaped()?;
            } else if is_ident(c) {
                self.consume()?;
            } else {
                return Some(());
            }
        }
    }

    fn consume_escaped(&mut self) -> Option<()> {
        if is_hex_digit(self.cur()?) {
            for _ in 1..5 {
                if !is_hex_digit(self.consume()?) {
                    break;
                }
            }
            if is_white_space(self.cur()?) {
                self.consume()?;
            }
        } else {
            self.consume()?;
        }
        Some(())
    }

    fn consume_ident_like<T: Handler>(&mut self, handler: &mut T) -> Option<()> {
        let start = self.cur_pos()?;
        self.consume_ident_sequence()?;
        let peek_pos = self.peek_pos()?;
        if self.cur_pos()? == start + 3
            && self.slice(start, peek_pos)?.to_ascii_lowercase() == "url("
        {
            while is_white_space(self.consume()?) {}
            let c = self.cur()?;
            if c == C_QUOTATION_MARK || c == C_APOSTROPHE {
                handler.function(self, start, peek_pos)
            } else {
                self.consume_url(handler, start)
            }
        } else if self.cur()? == C_LEFT_PARENTHESIS {
            self.consume()?;
            handler.function(self, start, self.cur_pos()?)
        } else {
            handler.ident(self, start, self.cur_pos()?)
        }
    }

    fn consume_url<T: Handler>(&mut self, handler: &mut T, start: usize) -> Option<()> {
        let content_start = self.cur_pos()?;
        loop {
            let c = self.cur()?;
            if c == C_REVERSE_SOLIDUS {
                self.consume()?;
                self.consume()?;
            } else if is_white_space(c) {
                let content_end = self.cur_pos()?;
                while is_white_space(self.consume()?) {}
                if self.cur()? != C_RIGHT_PARENTHESIS {
                    return Some(());
                }
                self.consume()?;
                return handler.url(self, start, self.cur_pos()?, content_start, content_end);
            } else if c == C_RIGHT_PARENTHESIS {
                let content_end = self.cur_pos()?;
                self.consume()?;
                return handler.url(self, start, self.cur_pos()?, content_start, content_end);
            } else if c == C_LEFT_PARENTHESIS {
                return Some(());
            } else {
                self.consume()?;
            }
        }
    }

    fn consume_string<T: Handler>(&mut self, handler: &mut T, end: char) -> Option<()> {
        let start = self.cur_pos()?;
        while let Some(c) = self.consume() {
            if c == end {
                self.consume()?;
                break;
            }
            if is_new_line(c) {
                break;
            }
            if c == C_REVERSE_SOLIDUS {
                let c2 = self.consume()?;
                if is_new_line(c2) {
                    self.consume()?;
                } else if are_valid_escape(c, c2) {
                    self.consume_escaped()?;
                }
            }
        }
        handler.string(self, start, self.cur_pos()?)
    }

    fn consume_number_sign<T: Handler>(&mut self, handler: &mut T) -> Option<()> {
        let c2 = self.peek()?;
        if is_ident(c2) || are_valid_escape(c2, self.peek2()?) {
            let start = self.cur_pos()?;
            let c = self.consume()?;
            if handler.is_selector(self)? && start_ident_sequence(c, self.peek()?, self.peek2()?) {
                self.consume_ident_sequence()?;
                return handler.id(self, start, self.cur_pos()?);
            }
        } else {
            return self.consume_delim();
        }
        Some(())
    }

    fn consume_left_parenthesis<T: Handler>(&mut self, handler: &mut T) -> Option<()> {
        self.consume()?;
        let end = self.cur_pos()?;
        handler.left_parenthesis(self, end - 1, end)
    }

    fn consume_right_parenthesis<T: Handler>(&mut self, handler: &mut T) -> Option<()> {
        self.consume()?;
        let end = self.cur_pos()?;
        handler.right_parenthesis(self, end - 1, end)
    }

    fn consume_plus_sign(&mut self) -> Option<()> {
        if start_number(self.cur()?, self.peek()?, self.peek2()?) {
            self.consume_numeric_token()
        } else {
            self.consume_delim()
        }
    }

    fn consume_comma<T: Handler>(&mut self, handler: &mut T) -> Option<()> {
        self.consume()?;
        let end = self.cur_pos()?;
        handler.comma(self, end - 1, end)
    }

    fn consume_minus<T: Handler>(&mut self, handler: &mut T) -> Option<()> {
        let c = self.cur()?;
        let c2 = self.peek()?;
        let c3 = self.peek2()?;
        if start_number(c, c2, c3) {
            self.consume_numeric_token()
        } else if c2 == C_HYPHEN_MINUS && c3 == C_GREATER_THAN_SIGN {
            self.consume()?;
            self.consume()?;
            Some(())
        } else if start_ident_sequence(c, c2, c3) {
            self.consume_ident_like(handler)
        } else {
            self.consume_delim()
        }
    }

    fn consume_full_stop<T: Handler>(&mut self, handler: &mut T) -> Option<()> {
        let c = self.cur()?;
        let c2 = self.peek()?;
        let c3 = self.peek2()?;
        if start_number(c, c2, c3) {
            return self.consume_numeric_token();
        }
        let start = self.cur_pos()?;
        self.consume()?;
        if !handler.is_selector(self)? || !start_ident_sequence(c2, c3, self.peek2()?) {
            return self.consume_delim();
        }
        self.consume_ident_sequence()?;
        handler.class(self, start, self.cur_pos()?)
    }

    fn consume_potential_pseudo<T: Handler>(&mut self, handler: &mut T) -> Option<()> {
        let start = self.cur_pos()?;
        let c = self.consume()?;
        if !handler.is_selector(self)? || !start_ident_sequence(c, self.peek()?, self.peek2()?) {
            return Some(());
        }
        self.consume_ident_sequence()?;
        if self.cur()? == C_LEFT_PARENTHESIS {
            self.consume()?;
            handler.pseudo_function(self, start, self.cur_pos()?)
        } else {
            handler.pseudo_class(self, start, self.cur_pos()?)
        }
    }

    fn consume_semicolon<T: Handler>(&mut self, handler: &mut T) -> Option<()> {
        self.consume()?;
        let end = self.cur_pos()?;
        handler.semicolon(self, end - 1, end)
    }

    fn consume_less_than_sign(&mut self) -> Option<()> {
        if self.consume()? == '!' && self.peek()? == '-' && self.peek2()? == '-' {
            self.consume()?;
            self.consume()?;
            self.consume()?;
        }
        Some(())
    }

    fn consume_at_sign<T: Handler>(&mut self, handler: &mut T) -> Option<()> {
        let start = self.cur_pos()?;
        let c = self.consume()?;
        if start_ident_sequence(c, self.peek()?, self.peek2()?) {
            self.consume_ident_sequence()?;
            return handler.at_keyword(self, start, self.cur_pos()?);
        }
        Some(())
    }

    fn consume_reverse_solidus<T: Handler>(&mut self, handler: &mut T) -> Option<()> {
        if are_valid_escape(self.cur()?, self.peek()?) {
            self.consume_ident_like(handler)
        } else {
            self.consume_delim()
        }
    }

    fn consume_left_curly<T: Handler>(&mut self, handler: &mut T) -> Option<()> {
        self.consume()?;
        let end = self.cur_pos()?;
        handler.left_curly(self, end - 1, end)
    }

    fn consume_right_curly<T: Handler>(&mut self, handler: &mut T) -> Option<()> {
        self.consume()?;
        let end = self.cur_pos()?;
        handler.right_curly(self, end - 1, end)
    }
}

fn is_new_line(c: char) -> bool {
    c == C_LINE_FEED || c == C_CARRIAGE_RETURN || c == C_FORM_FEED
}

fn is_space(c: char) -> bool {
    c == C_TAB || c == C_SPACE
}

fn is_white_space(c: char) -> bool {
    is_new_line(c) || is_space(c)
}

fn is_digit(c: char) -> bool {
    c >= C_0 && c <= C_9
}

fn is_hex_digit(c: char) -> bool {
    is_digit(c) || (c >= C_UPPER_A && c <= C_UPPER_F) || (c >= C_LOWER_A && c <= C_LOWER_F)
}

fn is_ident_start(c: char) -> bool {
    c == C_LOW_LINE
        || (c >= C_LOWER_A && c <= C_LOWER_Z)
        || (c >= C_UPPER_A && c <= C_UPPER_Z)
        || c > '\u{80}'
}

fn is_ident(c: char) -> bool {
    is_ident_start(c) || is_digit(c) || c == C_HYPHEN_MINUS
}

fn start_ident_sequence(c1: char, c2: char, c3: char) -> bool {
    if c1 == C_HYPHEN_MINUS {
        is_ident_start(c2) || c2 == C_HYPHEN_MINUS || are_valid_escape(c2, c3)
    } else {
        is_ident_start(c1) || are_valid_escape(c1, c2)
    }
}

fn are_valid_escape(c1: char, c2: char) -> bool {
    c1 == C_REVERSE_SOLIDUS && !is_new_line(c2)
}

fn start_number(c1: char, c2: char, c3: char) -> bool {
    if c1 == C_PLUS_SIGN || c1 == C_HYPHEN_MINUS {
        is_digit(c2) || (c2 == C_FULL_STOP && is_digit(c3))
    } else {
        is_digit(c1) || (c1 == C_FULL_STOP && is_digit(c2))
    }
}

#[cfg(test)]
mod tests {
    use indoc::indoc;

    use super::*;

    #[derive(Default)]
    struct SnapshotHandler {
        results: Vec<(String, String)>,
    }

    impl SnapshotHandler {
        pub fn add(&mut self, key: &str, value: &str) {
            self.results.push((key.to_string(), value.to_string()))
        }

        pub fn snapshot(&self) -> String {
            self.results
                .iter()
                .map(|(k, v)| format!("{k}: {v}\n"))
                .collect::<String>()
        }
    }

    impl Handler for SnapshotHandler {
        fn function(&mut self, lexer: &mut Lexer, start: usize, end: usize) -> Option<()> {
            self.add("function", lexer.slice(start, end)?);
            Some(())
        }

        fn ident(&mut self, lexer: &mut Lexer, start: usize, end: usize) -> Option<()> {
            self.add("ident", lexer.slice(start, end)?);
            Some(())
        }

        fn url(
            &mut self,
            lexer: &mut Lexer,
            _: usize,
            _: usize,
            content_start: usize,
            content_end: usize,
        ) -> Option<()> {
            self.add("url", lexer.slice(content_start, content_end)?);
            Some(())
        }

        fn string(&mut self, lexer: &mut Lexer, start: usize, end: usize) -> Option<()> {
            self.add("string", lexer.slice(start, end)?);
            Some(())
        }

        fn is_selector(&mut self, _: &mut Lexer) -> Option<bool> {
            Some(true)
        }

        fn id(&mut self, lexer: &mut Lexer, start: usize, end: usize) -> Option<()> {
            self.add("id", lexer.slice(start, end)?);
            Some(())
        }

        fn left_parenthesis(&mut self, lexer: &mut Lexer, start: usize, end: usize) -> Option<()> {
            self.add("left_parenthesis", lexer.slice(start, end)?);
            Some(())
        }

        fn right_parenthesis(&mut self, lexer: &mut Lexer, start: usize, end: usize) -> Option<()> {
            self.add("right_parenthesis", lexer.slice(start, end)?);
            Some(())
        }

        fn comma(&mut self, lexer: &mut Lexer, start: usize, end: usize) -> Option<()> {
            self.add("comma", lexer.slice(start, end)?);
            Some(())
        }

        fn class(&mut self, lexer: &mut Lexer, start: usize, end: usize) -> Option<()> {
            self.add("class", lexer.slice(start, end)?);
            Some(())
        }

        fn pseudo_function(&mut self, lexer: &mut Lexer, start: usize, end: usize) -> Option<()> {
            self.add("pseudo_function", lexer.slice(start, end)?);
            Some(())
        }

        fn pseudo_class(&mut self, lexer: &mut Lexer, start: usize, end: usize) -> Option<()> {
            self.add("pseudo_class", lexer.slice(start, end)?);
            Some(())
        }

        fn semicolon(&mut self, lexer: &mut Lexer, start: usize, end: usize) -> Option<()> {
            self.add("semicolon", lexer.slice(start, end)?);
            Some(())
        }

        fn at_keyword(&mut self, lexer: &mut Lexer, start: usize, end: usize) -> Option<()> {
            self.add("at_keyword", lexer.slice(start, end)?);
            Some(())
        }

        fn left_curly(&mut self, lexer: &mut Lexer, start: usize, end: usize) -> Option<()> {
            self.add("left_curly", lexer.slice(start, end)?);
            Some(())
        }

        fn right_curly(&mut self, lexer: &mut Lexer, start: usize, end: usize) -> Option<()> {
            self.add("right_curly", lexer.slice(start, end)?);
            Some(())
        }
    }

    #[test]
    fn parse_urls() {
        let mut h = SnapshotHandler::default();
        let mut l = Lexer::from(indoc! {r#"
            body {
                background: url(
                    https://example\2f4a8f.com\
            /image.png
                )
            }
            --element\ name.class\ name#_id {
                background: url(  "https://example.com/some url \"with\" 'spaces'.png"   )  url('https://example.com/\'"quotes"\'.png');
            }
        "#});
        l.lex(&mut h);
        assert!(l.cur().is_none());
        assert_eq!(
            h.snapshot(),
            indoc! {r#"
                ident: body
                left_curly: {
                ident: background
                url: https://example\2f4a8f.com\
                /image.png
                right_curly: }
                ident: --element\ name
                class: .class\ name
                id: #_id
                left_curly: {
                ident: background
                function: url(
                string: "https://example.com/some url \"with\" 'spaces'.png"
                right_parenthesis: )
                function: url(
                string: 'https://example.com/\'"quotes"\'.png'
                right_parenthesis: )
                semicolon: ;
                right_curly: }
            "#}
        );
    }

    #[test]
    fn parse_pseudo_functions() {
        let mut h = SnapshotHandler::default();
        let mut l = Lexer::from(indoc! {r#"
            :local(.class#id, .class:not(*:hover)) { color: red; }
            :import(something from ":somewhere") {}
        "#});
        l.lex(&mut h);
        assert!(l.cur().is_none());
        assert_eq!(
            h.snapshot(),
            indoc! {r#"
                pseudo_function: :local(
                class: .class
                id: #id
                comma: ,
                class: .class
                pseudo_function: :not(
                pseudo_class: :hover
                right_parenthesis: )
                right_parenthesis: )
                left_curly: {
                ident: color
                ident: red
                semicolon: ;
                right_curly: }
                pseudo_function: :import(
                ident: something
                ident: from
                string: ":somewhere"
                right_parenthesis: )
                left_curly: {
                right_curly: }
            "#}
        );
    }

    #[test]
    fn parse_at_rules() {
        let mut h = SnapshotHandler::default();
        let mut l = Lexer::from(indoc! {r#"
            @media (max-size: 100px) {
                @import "external.css";
                body { color: red; }
            }
        "#});
        l.lex(&mut h);
        assert!(l.cur().is_none());
        println!("{}", h.snapshot());
        assert_eq!(
            h.snapshot(),
            indoc! {r#"
                at_keyword: @media
                left_parenthesis: (
                ident: max-size
                right_parenthesis: )
                left_curly: {
                at_keyword: @import
                string: "external.css"
                semicolon: ;
                ident: body
                left_curly: {
                ident: color
                ident: red
                semicolon: ;
                right_curly: }
                right_curly: }
            "#}
        );
    }
}
