use super::CollectOptions;
use super::Dependency;
use super::DependencyContext;
use super::DependencyContextCapacity;
use super::ImportAttributes;
use super::Mode;
use crate::collect_dependencies;
use crate::collect_dependencies_with_options;

const BOOTSTRAP: &str = include_str!("../../fixtures/bootstrap.min.css");
const RSPACK: &str = include_str!("../../fixtures/rspack-css-modules.css");

#[test]
fn estimated_capacities_are_reserved_eagerly_and_cover_fixtures() {
    let estimate = DependencyContextCapacity::estimate(
        BOOTSTRAP.len(),
        Mode::Local,
        CollectOptions::default(),
    );
    let (bootstrap, _) = collect_dependencies(BOOTSTRAP, Mode::Local);
    assert!(bootstrap.dependencies.capacity() >= estimate.dependencies);
    assert!(bootstrap.import_attributes.capacity() >= estimate.import_attributes as usize);
    assert!(
        bootstrap.composes_local_classes.capacity() >= estimate.composes_local_classes as usize
    );
    assert!(bootstrap.composes_names.capacity() >= estimate.composes_names as usize);
    assert!(
        bootstrap.value_at_rule_import_items.capacity()
            >= estimate.value_at_rule_import_items as usize
    );

    for mode in [Mode::Local, Mode::Global, Mode::Pure] {
        let estimate =
            DependencyContextCapacity::estimate(RSPACK.len(), mode, CollectOptions::default());
        let (context, _) = collect_dependencies(RSPACK, mode);
        assert!(context.dependencies.len() <= estimate.dependencies);
        assert!(context.import_attributes.len() <= estimate.import_attributes as usize);
        assert!(context.composes_local_classes.len() <= estimate.composes_local_classes as usize);
        assert!(context.composes_names.len() <= estimate.composes_names as usize);
        assert!(
            context.value_at_rule_import_items.len()
                <= estimate.value_at_rule_import_items as usize
        );
    }

    let options = CollectOptions {
        collect_dashed_idents: true,
    };
    let estimate = DependencyContextCapacity::estimate(RSPACK.len(), Mode::Local, options);
    let (context, _) = collect_dependencies_with_options(RSPACK, Mode::Local, options);
    assert!(context.dashed_ident_occurrences.capacity() >= estimate.dashed_ident_occurrences);
    assert!(context.dashed_ident_occurrences.len() <= estimate.dashed_ident_occurrences);
}

#[test]
#[cfg(target_pointer_width = "64")]
fn dependency_keeps_rare_import_payload_out_of_hot_storage() {
    assert_eq!(std::mem::size_of::<DependencyContext<'static>>(), 144);
    assert_eq!(std::mem::size_of::<Dependency<'static>>(), 48);
    assert_eq!(std::mem::size_of::<ImportAttributes<'static>>(), 48);
}
