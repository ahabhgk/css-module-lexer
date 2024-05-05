# css-module-lexer

Lexes CSS modules returning their dependencies metadata.

- Blazing fast: no parsing, no AST creation, only lexing, minimal heap allocation.
- Error tolerant: uninterrupted by bad syntax, no errors, only warnings.
- Syntax rich: supports CSS, CSS Modules, and iCSS.

## Roadmap

- [x] CSS:
  - [x] @import
  - [x] url(), image-set()
- [ ] CSS Modules
  - [x] :local, :local(), :global, :global()
  - [x] local scope by default
  - [x] nesting
  - [x] var(), @property
  - [x] @keyframe
  - [x] composes
  - [ ] @values
  - [ ] more warnings
- [ ] iCSS
  - [x] :export
  - [ ] :import
