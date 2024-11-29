use css_module_lexer::{collect_dependencies, Mode};

fn main() {
    let Some(path) = std::env::args().nth(1) else {
        eprintln!("USAGE: cli <path>");
        return;
    };
    let Ok(input) = std::fs::read_to_string(&path) else {
        eprintln!("Failed to read file: {}", path);
        return;
    };
    let (dependencies, warnings) = collect_dependencies(&input, Mode::Css);
    if dependencies.is_empty() {
        println!("No dependencies found");
    } else {
        println!("Dependencies:");
        for dependency in dependencies {
            println!("{:?}", dependency);
        }
    }
    if warnings.is_empty() {
        println!("No warnings found");
    } else {
        println!("Warnings:");
        for warning in warnings {
            println!("{:?}", warning);
        }
    }
}
