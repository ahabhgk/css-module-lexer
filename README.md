# css-module-lexer

Lexes CSS modules returning their dependencies metadata.

- Blazing fast: no parsing, no AST creation, only lexing, minimal heap allocation.
- Error tolerant: uninterrupted by bad syntax, no errors, only warnings.
- Syntax rich: supports CSS, iCSS, and CSS Modules.

## Roadmap

- [x] CSS:
  - [x] @import
  - [x] url(), image-set()
- [x] iCSS
  - [x] :import
  - [x] :export
- [ ] CSS Modules
  - [x] :local, :local(), :global, :global()
  - [x] local scope by default
  - [x] nesting
  - [x] var(), @property
  - [x] @keyframe
  - [x] composes
  - [ ] @values
