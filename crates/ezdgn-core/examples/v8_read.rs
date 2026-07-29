use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;

use ezdgn_core::{read_v8, V8ScanOptions};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::from(2)
        }
    }
}

fn run() -> Result<(), String> {
    let path = parse_path()?;
    let input =
        fs::read(&path).map_err(|error| format!("failed to read {}: {error}", path.display()))?;
    let document = read_v8(&input, V8ScanOptions::default())
        .map_err(|error| format!("failed to read {}: {error}", path.display()))?;

    println!("file={}", path.display());
    println!("model_count={}", document.models.len());
    for model in &document.models {
        println!("\nmodel_index={}", model.metadata.index);
        println!("name={:?}", model.metadata.name);
        println!("description={:?}", model.metadata.description);
        println!("dimension={}", model.metadata.dimension.value());
        println!("uor_per_master={}", model.metadata.uor_per_master);
        println!("master_unit={:?}", model.metadata.master_unit);
        println!("sub_unit={:?}", model.metadata.sub_unit);
        println!("raw_element_count={}", model.elements.len());
        println!("entity_count={}", model.entities().count());
        println!(
            "auxiliary_record_count={}",
            model
                .elements
                .iter()
                .map(|element| element.auxiliary_records.len())
                .sum::<usize>()
        );
        let mut kinds = BTreeMap::<&str, usize>::new();
        for element in &model.elements {
            *kinds.entry(element.kind()).or_default() += 1;
        }
        println!("element_kind_counts:");
        for (kind, count) in kinds {
            println!("{kind}\t{count}");
        }
    }
    Ok(())
}

fn parse_path() -> Result<PathBuf, String> {
    let mut arguments = env::args_os().skip(1);
    let path = arguments
        .next()
        .map(PathBuf::from)
        .ok_or_else(|| "usage: v8_read FILE".to_owned())?;
    if arguments.next().is_some() {
        return Err("usage: v8_read FILE".to_owned());
    }
    Ok(path)
}
