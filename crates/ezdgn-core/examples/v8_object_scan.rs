use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;

use ezdgn_core::{scan_v8_objects, V8ScanOptions};

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
    let document = scan_v8_objects(&input, V8ScanOptions::default())
        .map_err(|error| format!("failed to scan {}: {error}", path.display()))?;

    println!("file={}", path.display());
    println!("model_count={}", document.models.len());
    println!(
        "graphical_object_count={}",
        document.graphical_object_count()
    );
    println!("total_object_count={}", document.total_object_count());
    println!("total_inflated_bytes={}", document.total_inflated_bytes);
    println!("named_page_count={}", document.named_pages.len());
    println!(
        "named_auxiliary_record_count={}",
        document
            .named_auxiliary_pages
            .iter()
            .map(|page| page.records.len())
            .sum::<usize>()
    );

    for model in &document.models {
        println!("\nmodel_index={}", model.index.index);
        println!("storage_path={}", model.storage_path);
        println!("storage_index={}", model.index.storage_index);
        println!("model_number={}", model.index.model_number);
        println!("model_id={}", model.index.model_id);
        println!("name={:?}", model.index.name);
        println!("description={:?}", model.index.description);
        println!("graphical_page_count={}", model.graphical_pages.len());
        println!("control_page_count={}", model.control_pages.len());
        println!(
            "graphical_auxiliary_record_count={}",
            model
                .graphical_auxiliary_pages
                .iter()
                .map(|page| page.records.len())
                .sum::<usize>()
        );

        let mut types = BTreeMap::<u16, usize>::new();
        for object in model.graphical_objects() {
            *types.entry(object.element_type).or_default() += 1;
        }
        println!("element_type_counts:");
        for (element_type, count) in types {
            println!("{element_type}\t{count}");
        }
    }
    Ok(())
}

fn parse_path() -> Result<PathBuf, String> {
    let mut arguments = env::args_os().skip(1);
    let path = arguments
        .next()
        .map(PathBuf::from)
        .ok_or_else(|| "usage: v8_object_scan FILE".to_owned())?;
    if arguments.next().is_some() {
        return Err("usage: v8_object_scan FILE".to_owned());
    }
    Ok(path)
}
