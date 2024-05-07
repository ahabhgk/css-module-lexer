use css_module_lexer::collect_css_modules_dependencies;
use css_module_lexer::Dependency;
use css_module_lexer::Range;

use crate::slice_range;

fn modules_local_by_default(input: &str) -> String {
    let (dependencies, _) = collect_css_modules_dependencies(input);
    let mut result = String::new();
    let mut index = 0;
    for dependency in dependencies {
        match dependency {
            Dependency::LocalIdent { name, range }
            | Dependency::LocalKeyframesDecl { name, range }
            | Dependency::LocalKeyframes { name, range } => {
                result += slice_range(input, &Range::new(index, range.start)).unwrap();
                result += ":local(";
                result += name;
                result += ")";
                index = range.end;
            }
            Dependency::Replace { content, range } => {
                result += slice_range(input, &Range::new(index, range.start)).unwrap();
                result += content;
                index = range.end;
            }
            _ => {}
        }
    }
    let len = input.len() as u32;
    if index != len {
        result += slice_range(input, &Range::new(index, len)).unwrap();
    }
    result
}

fn test(input: &str, expected: &str) {
    let actual = modules_local_by_default(input);
    assert_eq!(expected, actual);
}

#[test]
fn scope_selectors() {
    test(".foobar {}", ":local(.foobar) {}");
}

#[test]
fn scope_escaped_selectors() {
    test(".\\3A \\) {}", ":local(.\\3A \\)) {}");
}

#[test]
fn scope_ids() {
    test("#foobar {}", ":local(#foobar) {}");
}

#[test]
fn scope_escaped_ids() {
    test("#\\#test {}", ":local(#\\#test) {}");
    test("#u-m\\00002b {}", ":local(#u-m\\00002b) {}");
}

#[test]
fn scope_multiple_selectors() {
    test(".foo, .baz {}", ":local(.foo), :local(.baz) {}");
}

#[test]
fn scope_sibling_selectors() {
    test(".foo ~ .baz {}", ":local(.foo) ~ :local(.baz) {}");
}

#[test]
fn scope_psuedo_elements() {
    test(".foo:after {}", ":local(.foo):after {}");
}

#[test]
fn scope_media_queries() {
    test(
        "@media only screen { .foo {} }",
        "@media only screen { :local(.foo) {} }",
    );
}

#[test]
fn allow_narrow_global_selectors() {
    test(":global(.foo .bar) {}", ".foo .bar {}");
}

#[test]
fn allow_narrow_local_selectors() {
    test(":local(.foo .bar) {}", ":local(.foo) :local(.bar) {}");
}

#[test]
fn allow_broad_global_selectors() {
    test(":global .foo .bar {}", ".foo .bar {}");
}

#[test]
fn allow_broad_local_selectors() {
    test(":local .foo .bar {}", ":local(.foo) :local(.bar) {}");
}

#[test]
fn allow_multiple_narrow_global_selectors() {
    test(":global(.foo), :global(.bar) {}", ".foo, .bar {}");
}

#[test]
fn allow_multiple_broad_global_selectors() {
    test(":global .foo, :global .bar {}", ".foo, .bar {}");
}

#[test]
fn allow_multiple_broad_local_selectors() {
    test(
        ":local .foo, :local .bar {}",
        ":local(.foo), :local(.bar) {}",
    );
}

#[test]
fn allow_narrow_global_selectors_nested_inside_local_styles() {
    test(".foo :global(.foo .bar) {}", ":local(.foo) .foo .bar {}");
}

#[test]
fn allow_broad_global_selectors_nested_inside_local_styles() {
    test(".foo :global .foo .bar {}", ":local(.foo) .foo .bar {}");
}

#[test]
fn allow_parentheses_inside_narrow_global_selectors() {
    test(
        ".foo :global(.foo:not(.bar)) {}",
        ":local(.foo) .foo:not(.bar) {}",
    );
}

#[test]
fn allow_parentheses_inside_narrow_local_selectors() {
    test(
        ".foo :local(.foo:not(.bar)) {}",
        ":local(.foo) :local(.foo):not(:local(.bar)) {}",
    );
}

#[test]
fn allow_narrow_global_selectors_appended_to_local_styles() {
    test(".foo:global(.foo.bar) {}", ":local(.foo).foo.bar {}");
}

#[test]
fn ignore_selectors_that_are_already_local() {
    test(":local(.foobar) {}", ":local(.foobar) {}");
}

#[test]
fn ignore_nested_selectors_that_are_already_local() {
    test(
        ":local(.foo) :local(.bar) {}",
        ":local(.foo) :local(.bar) {}",
    );
}

#[test]
fn ignore_multiple_selectors_that_are_already_local() {
    test(
        ":local(.foo), :local(.bar) {}",
        ":local(.foo), :local(.bar) {}",
    );
}

#[test]
fn ignore_sibling_selectors_that_are_already_local() {
    test(
        ":local(.foo) ~ :local(.bar) {}",
        ":local(.foo) ~ :local(.bar) {}",
    );
}

#[test]
fn ignore_psuedo_elements_that_are_already_local() {
    test(":local(.foo):after {}", ":local(.foo):after {}");
}

#[test]
fn trim_whitespace_after_empty_broad_selector() {
    test(".bar :global :global {}", ":local(.bar) {}");
}

#[test]
fn broad_global_should_be_limited_to_selector() {
    test(
        ":global .foo, .bar :global, .foobar :global {}",
        // should be ".foo, :local(.bar), :local(.foobar) {}", but the whitespace is fine
        ".foo, :local(.bar) , :local(.foobar) {}",
    );
}

#[test]
fn broad_global_should_be_limited_to_nested_selector() {
    test(
        ".foo:not(:global .bar).foobar {}",
        ":local(.foo):not(.bar):local(.foobar) {}",
    );
}

#[test]
fn broad_global_and_local_should_allow_switching() {
    test(
        ".foo :global .bar :local .foobar :local .barfoo {}",
        ":local(.foo) .bar :local(.foobar) :local(.barfoo) {}",
    );
}

#[test]
fn localize_a_single_animation_name() {
    test(
        ".foo { animation-name: bar; }",
        ":local(.foo) { animation-name: :local(bar); }",
    );
}

#[test]
fn not_localize_animation_name_in_a_var_function() {
    test(
        ".foo { animation-name: var(--bar); }",
        ":local(.foo) { animation-name: var(--bar); }",
    );
    test(
        ".foo { animation-name: vAr(--bar); }",
        ":local(.foo) { animation-name: vAr(--bar); }",
    );
}

#[test]
fn not_localize_animation_name_in_an_env_function() {
    test(
        ".foo { animation-name: env(bar); }",
        ":local(.foo) { animation-name: env(bar); }",
    );
    test(
        ".foo { animation-name: eNv(bar); }",
        ":local(.foo) { animation-name: eNv(bar); }",
    );
}

#[test]
fn not_localize_a_single_animation_delay() {
    test(
        ".foo { animation-delay: 1s; }",
        ":local(.foo) { animation-delay: 1s; }",
    );
}

#[test]
fn localize_multiple_animation_names() {
    test(
        ".foo { animation-name: bar, foobar; }",
        ":local(.foo) { animation-name: :local(bar), :local(foobar); }",
    );
}

#[test]
fn not_localize_revert() {
    test(
        ".foo { animation: revert; }",
        ":local(.foo) { animation: revert; }",
    );
    test(
        ".foo { animation-name: revert; }",
        ":local(.foo) { animation-name: revert; }",
    );
    test(
        ".foo { animation-name: revert, foo, none; }",
        ":local(.foo) { animation-name: revert, :local(foo), none; }",
    );
}

#[test]
fn not_localize_revert_layer() {
    test(
        ".foo { animation: revert-layer; }",
        ":local(.foo) { animation: revert-layer; }",
    );
    test(
        ".foo { animation-name: revert-layer; }",
        ":local(.foo) { animation-name: revert-layer; }",
    );
}

#[test]
fn localize_animation_using_special_characters() {
    test(
        ".foo { animation: \\@bounce; }",
        ":local(.foo) { animation: :local(\\@bounce); }",
    );
    test(
        ".foo { animation: bou\\@nce; }",
        ":local(.foo) { animation: :local(bou\\@nce); }",
    );
    test(
        ".foo { animation: \\ as; }",
        ":local(.foo) { animation: :local(\\ as); }",
    );
    test(
        ".foo { animation: t\\ t; }",
        ":local(.foo) { animation: :local(t\\ t); }",
    );
    test(
        ".foo { animation: -\\a; }",
        ":local(.foo) { animation: :local(-\\a); }",
    );
    test(
        ".foo { animation: --\\a; }",
        ":local(.foo) { animation: :local(--\\a); }",
    );
    test(
        ".foo { animation: \\a; }",
        ":local(.foo) { animation: :local(\\a); }",
    );
    test(
        ".foo { animation: -\\a; }",
        ":local(.foo) { animation: :local(-\\a); }",
    );
    test(
        ".foo { animation: --; }",
        ":local(.foo) { animation: :local(--); }",
    );
    test(
        ".foo { animation: 😃bounce😃; }",
        ":local(.foo) { animation: :local(😃bounce😃); }",
    );
    test(
        ".foo { animation: --foo; }",
        ":local(.foo) { animation: :local(--foo); }",
    );
}

#[test]
fn not_localize_name_in_nested_function() {
    test(
        ".foo { animation: fade .2s var(--easeOutQuart) .1s forwards }",
        ":local(.foo) { animation: :local(fade) .2s var(--easeOutQuart) .1s forwards }",
    );
    test(
        ".foo { animation: fade .2s env(FOO_BAR) .1s forwards, name }",
        ":local(.foo) { animation: :local(fade) .2s env(FOO_BAR) .1s forwards, :local(name) }",
    );
    test(
        ".foo { animation: var(--foo-bar) .1s forwards, name }",
        ":local(.foo) { animation: var(--foo-bar) .1s forwards, :local(name) }",
    );
    test(
        ".foo { animation: var(--foo-bar) .1s forwards name, name }",
        ":local(.foo) { animation: var(--foo-bar) .1s forwards :local(name), :local(name) }",
    );
}

#[test]
fn localize_animation() {
    test(
        ".foo { animation: a; }",
        ":local(.foo) { animation: :local(a); }",
    );
    test(
        ".foo { animation: bar 5s, foobar; }",
        ":local(.foo) { animation: :local(bar) 5s, :local(foobar); }",
    );
    test(
        ".foo { animation: ease ease; }",
        ":local(.foo) { animation: ease :local(ease); }",
    );
    test(
        ".foo { animation: 0s ease 0s 1 normal none test running; }",
        ":local(.foo) { animation: 0s ease 0s 1 normal none :local(test) running; }",
    );
}
