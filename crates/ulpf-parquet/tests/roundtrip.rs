use std::path::PathBuf;

use parquet::file::reader::{FileReader, SerializedFileReader};
use parquet::record::RowAccessor;
use ulpf_parquet::{ParquetWriter, Row};

fn tmp(name: &str) -> PathBuf {
    let mut p = std::env::temp_dir();
    p.push(format!("ulpf-parquet-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn line(i: i64) -> String {
    format!("{{\"class_uid\":4001,\"ulpf\":{{\"raw_id\":{i}}}}}")
}

#[test]
fn every_row_comes_back_with_every_column() {
    let dir = tmp("roundtrip");
    let path = dir.join("events.parquet");
    let n = 5000i64;
    let lines: Vec<String> = (0..n).map(line).collect();
    let mut w = ParquetWriter::create(&path, 1024).unwrap();
    for i in 0..n {
        w.push(Row {
            raw_id: i,
            time_ms: 1_757_000_000_000 + i,
            // every third row has no parser, so the null path is exercised
            parser: (i % 3 != 0).then_some("cisco_asa"),
            source: "fw/syslog.log",
            class_uid: 4001,
            normalized: lines[i as usize].as_bytes(),
            src_ip: (i % 2 == 0).then_some("10.0.0.1"),
            dst_ip: Some("192.0.2.9"),
            user: (i % 5 == 0).then_some("alice"),
            device: Some("edge-fw-1"),
            dst_port: (i % 2 == 0).then_some(443),
        });
        if i % 100 == 0 {
            w.end_batch().unwrap();
        }
    }
    // the part file is not readable output; only the rename makes it one
    assert!(path.with_extension("parquet.part").exists() || dir.join("events.parquet.part").exists());
    assert!(!path.exists(), "no footer yet, so no .parquet");
    let stats = w.finish().unwrap();
    assert_eq!(stats.rows, n as u64);
    assert_eq!(stats.files, 1);
    assert!(path.exists());
    assert!(!dir.join("events.parquet.part").exists());

    let reader = SerializedFileReader::new(std::fs::File::open(&path).unwrap()).unwrap();
    assert_eq!(reader.metadata().file_metadata().num_rows(), n);
    let mut seen = 0i64;
    for row in reader.get_row_iter(None).unwrap() {
        let row = row.unwrap();
        let i = row.get_long(0).unwrap();
        assert_eq!(i, seen);
        assert_eq!(row.get_timestamp_millis(1).unwrap(), 1_757_000_000_000 + i);
        match i % 3 {
            0 => assert!(row.get_string(2).is_err(), "null parser is null"),
            _ => assert_eq!(row.get_string(2).unwrap(), "cisco_asa"),
        }
        assert_eq!(row.get_string(3).unwrap(), "fw/syslog.log");
        assert_eq!(row.get_int(4).unwrap(), 4001);
        assert_eq!(row.get_string(5).unwrap(), &line(i));
        assert_eq!(row.get_string(7).unwrap(), "192.0.2.9");
        assert_eq!(row.get_string(9).unwrap(), "edge-fw-1");
        if i % 2 == 0 {
            assert_eq!(row.get_string(6).unwrap(), "10.0.0.1");
            assert_eq!(row.get_int(10).unwrap(), 443);
        } else {
            assert!(row.get_string(6).is_err());
            assert!(row.get_int(10).is_err());
        }
        if i % 5 == 0 {
            assert_eq!(row.get_string(8).unwrap(), "alice");
        }
        seen += 1;
    }
    assert_eq!(seen, n);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn roll_closes_one_file_and_opens_the_next() {
    let dir = tmp("roll");
    let a = dir.join("a.parquet");
    let b = dir.join("b.parquet");
    let l = line(0);
    let row = |i: i64| Row {
        raw_id: i,
        time_ms: 0,
        parser: None,
        source: "s",
        class_uid: 0,
        normalized: l.as_bytes(),
        src_ip: None,
        dst_ip: None,
        user: None,
        device: None,
        dst_port: None,
    };
    let mut w = ParquetWriter::create(&a, 8192).unwrap();
    for i in 0..10 {
        w.push(row(i));
    }
    let stats = w.roll(&b).unwrap();
    assert_eq!((stats.rows, stats.files), (10, 1));
    assert!(a.exists());
    for i in 10..15 {
        w.push(row(i));
    }
    let stats = w.finish().unwrap();
    assert_eq!((stats.rows, stats.files), (15, 2));
    let count = |p: &PathBuf| SerializedFileReader::new(std::fs::File::open(p).unwrap()).unwrap().metadata().file_metadata().num_rows();
    assert_eq!(count(&a), 10);
    assert_eq!(count(&b), 5);
    assert_eq!(std::fs::read_dir(&dir).unwrap().filter(|e| e.as_ref().unwrap().path().extension().is_some_and(|x| x == "part")).count(), 0);
    std::fs::remove_dir_all(&dir).unwrap();
}
