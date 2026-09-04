mod common;
use common::*;
use ulpf_parse::load_dir;

#[test]
fn malformed_files_are_reported_with_path_and_line_and_others_still_load() {
    let dir = std::env::temp_dir().join(format!("ulpf-parse-load-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let good = "[parser]\nname = \"good\"\nvendor = \"v\"\nproduct = \"p\"\n[match]\ncontains = [\"x\"]\n[strategy]\nkind = \"kv\"\n";
    std::fs::write(dir.join("a_good.toml"), good).unwrap();
    std::fs::write(dir.join("b_syntax.toml"), "[parser]\nname = \"broken\"\nvendor = \"v\n").unwrap();
    std::fs::write(dir.join("c_semantic.toml"), "[parser]\nname = \"sem\"\nvendor = \"v\"\nproduct = \"p\"\n[match]\ncontains = [\"x\"]\n[strategy]\nkind = \"pattern\"\npattern = \"{a:nosuchtype}\"\n").unwrap();
    std::fs::write(dir.join("d_dup.toml"), good).unwrap();
    std::fs::write(dir.join("e_unknown_key.toml"), "[parser]\nname = \"u\"\nvendor = \"v\"\nproduct = \"p\"\nocsf_class = 4001\n[match]\ncontains = [\"x\"]\n[strategy]\nkind = \"kv\"\n").unwrap();
    std::fs::write(dir.join("ignored.txt"), "not toml").unwrap();
    let report = load_dir(&dir).unwrap();
    assert_eq!(report.parsers.len(), 1);
    assert_eq!(report.parsers[0].name(), "good");
    let msgs: Vec<String> = report.errors.iter().map(|e| e.to_string()).collect();
    assert_eq!(report.errors.len(), 4, "{msgs:?}");
    let syntax = &report.errors[0];
    assert!(syntax.path.ends_with("b_syntax.toml"));
    assert_eq!(syntax.line, Some(3), "{syntax}");
    assert!(msgs[1].contains("nosuchtype"), "{}", msgs[1]);
    assert!(msgs[2].contains("duplicate parser name"), "{}", msgs[2]);
    assert!(msgs[3].contains("ocsf_class"), "schema vocabulary in a parser file must be rejected: {}", msgs[3]);
    std::fs::remove_dir_all(&dir).unwrap();
    assert!(load_dir(&dir).is_err(), "missing directory is the only hard error");
}

#[test]
fn repo_parsers_directory_loads_clean() {
    let report = load_dir(&repo().join("parsers")).unwrap();
    assert!(report.errors.is_empty(), "{:?}", report.errors.iter().map(|e| e.to_string()).collect::<Vec<_>>());
    assert!(report.parsers.len() >= 2);
}
