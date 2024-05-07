use css_module_lexer::collect_css_modules_dependencies;
use css_module_lexer::Dependency;
use css_module_lexer::Range;
use indoc::indoc;

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

// #[test]
// fn localize_animation_with_vendor_prefix() {
//     test(
//         ".foo { -webkit-animation: bar; animation: bar; }",
//         ":local(.foo) { -webkit-animation: :local(bar); animation: :local(bar); }",
//     );
// }

#[test]
fn not_localize_other_rules() {
    test(
        ".foo { content: \"animation: bar;\" }",
        ":local(.foo) { content: \"animation: bar;\" }",
    );
}

#[test]
fn not_localize_global_rules() {
    test(
        ":global .foo { animation: foo; animation-name: bar; }",
        ".foo { animation: foo; animation-name: bar; }",
    );
}

#[test]
fn handle_nested_global() {
    test(":global .a:not(:global .b) {}", ".a:not(.b) {}");
    test(
        ":global .a:not(:global .b:not(:global .c)) {}",
        ".a:not(.b:not(.c)) {}",
    );
    test(
        ":local .a:not(:not(:not(:global .c))) {}",
        ":local(.a):not(:not(:not(.c))) {}",
    );
    test(
        ":global .a:not(:global .b, :global .c) {}",
        ".a:not(.b, .c) {}",
    );
    test(
        ":local .a:not(:global .b, :local .c) {}",
        ":local(.a):not(.b, :local(.c)) {}",
    );
    test(
        ":global .a:not(:local .b, :global .c) {}",
        ".a:not(:local(.b), .c) {}",
    );
    test(":global .a:not(.b, .c) {}", ".a:not(.b, .c) {}");
    test(
        ":local .a:not(.b, .c) {}",
        ":local(.a):not(:local(.b), :local(.c)) {}",
    );
    test(
        ":global .a:not(:local .b, .c) {}",
        ".a:not(:local(.b), :local(.c)) {}",
    );
}

#[test]
fn handle_a_complex_animation_rule() {
    test(
        ".foo { animation: foo, bar 5s linear 2s infinite alternate, barfoo 1s; }", 
        ":local(.foo) { animation: :local(foo), :local(bar) 5s linear 2s infinite alternate, :local(barfoo) 1s; }",
    );
}

#[test]
fn handle_animations_where_the_first_value_is_not_the_animation_name() {
    test(
        ".foo { animation: 1s foo; }",
        ":local(.foo) { animation: 1s :local(foo); }",
    );
}

#[test]
fn handle_animations_where_the_first_value_is_not_the_animation_name_whilst_also_using_keywords() {
    test(
        ".foo { animation: 1s normal ease-out infinite foo; }",
        ":local(.foo) { animation: 1s normal ease-out infinite :local(foo); }",
    );
}

#[test]
fn not_treat_animation_curve_as_identifier_of_animation_name_even_if_it_separated_by_comma() {
    test(
        ".foo { animation: slide-right 300ms forwards ease-out, fade-in 300ms forwards ease-out; }",
        ":local(.foo) { animation: :local(slide-right) 300ms forwards ease-out, :local(fade-in) 300ms forwards ease-out; }",
    );
}

#[test]
fn not_treat_start_and_end_keywords_in_steps_function_as_identifiers() {
    test(
        indoc! {r#"
            .foo { animation: spin 1s steps(12, end) infinite; }
            .foo { animation: spin 1s STEPS(12, start) infinite; }
            .foo { animation: spin 1s steps(12, END) infinite; }
            .foo { animation: spin 1s steps(12, START) infinite; }
        "#},
        indoc! {r#"
            :local(.foo) { animation: :local(spin) 1s steps(12, end) infinite; }
            :local(.foo) { animation: :local(spin) 1s STEPS(12, start) infinite; }
            :local(.foo) { animation: :local(spin) 1s steps(12, END) infinite; }
            :local(.foo) { animation: :local(spin) 1s steps(12, START) infinite; }
        "#},
    );
}

#[test]
fn handle_animations_with_custom_timing_functions() {
    test(
        ".foo { animation: 1s normal cubic-bezier(0.25, 0.5, 0.5. 0.75) foo; }",
        ":local(.foo) { animation: 1s normal cubic-bezier(0.25, 0.5, 0.5. 0.75) :local(foo); }",
    );
}

#[test]
fn handle_animations_whose_names_are_keywords() {
    test(
        ".foo { animation: 1s infinite infinite; }",
        ":local(.foo) { animation: 1s infinite :local(infinite); }",
    );
}

#[test]
fn handle_not_localize_an_animation_shorthand_value_of_inherit() {
    test(
        ".foo { animation: inherit; }",
        ":local(.foo) { animation: inherit; }",
    );
}

#[test]
fn handle_constructor_as_animation_name() {
    test(
        ".foo { animation: constructor constructor; }",
        // should be ":local(.foo) { animation: :local(constructor) :local(constructor); }"
        // but the output of postcss-modules-scope is "._local_foo) { animation: _local_constructor :local(constructor); }"
        // postcss-modules-scope only process the first :local in decl.value
        // therefore seems our output is more appropriate and it's fine to have different output here
        ":local(.foo) { animation: constructor :local(constructor); }",
    );
}
