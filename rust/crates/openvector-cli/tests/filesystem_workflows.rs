use serde_json::Value;
use std::{
    path::Path,
    process::{Command, Output},
};

const IMAGE: &[u8] = include_bytes!("../../../../test/fixtures/recent30-artwork.png");

fn invoke(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_openvector"))
        .current_dir(root)
        .args(args)
        .output()
        .unwrap()
}

fn success(output: &Output) {
    assert!(
        output.status.success() && output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn failure(output: &Output, diagnostic: &str) {
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains(diagnostic), "{stderr}");
}

#[test]
fn directory_batch_dry_run_matches_conversion_and_leaves_profiles_untouched() {
    use std::fs;

    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    for folder in ["scans/nested", "scans/.hidden", "exports"] {
        fs::create_dir_all(root.join(folder)).unwrap();
    }
    for file in [
        "scans/logo.png",
        "scans/nested/logo.PNG",
        "scans/.hidden/logo.png",
        "scans/.thumbnail.png",
        "scans/readme.txt",
        "explicit.png",
    ] {
        fs::write(root.join(file), IMAGE).unwrap();
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink("logo.png", root.join("scans/linked.png")).unwrap();
        std::os::unix::fs::symlink("..", root.join("scans/nested/loop")).unwrap();
    }

    let profiles = root.join("profiles.json");
    let direct = invoke(root, &["batch", "scans", "--dry-run"]);
    failure(&direct, "--output-dir");
    let shallow = invoke(
        root,
        &["batch", "scans", "--output-dir", "exports", "--dry-run"],
    );
    success(&shallow);
    let shallow: Value = serde_json::from_slice(&shallow.stdout).unwrap();
    let shallow = shallow["files"].as_array().unwrap();
    assert_eq!(shallow.len(), 1);
    assert_eq!(
        shallow[0]["filename"].as_str().unwrap().replace('\\', "/"),
        "scans/logo.png"
    );
    assert_eq!(shallow[0]["outputFilename"], "exports/logo.ai");
    assert_eq!(shallow[0]["status"], "would-write");

    let mut args = vec![
        "batch",
        "scans",
        "explicit.png",
        "--recursive",
        "--format",
        "svg",
        "--output-dir",
        "exports",
        "--profile",
        "Default Advanced Settings",
        "--profiles-file",
        "profiles.json",
    ];
    args.push("--dry-run");
    let plan = invoke(root, &args);
    success(&plan);
    let plan: Value = serde_json::from_slice(&plan.stdout).unwrap();
    let normalized_sources: Vec<_> = plan["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|file| file["filename"].as_str().unwrap().replace('\\', "/"))
        .collect();
    assert_eq!(
        normalized_sources,
        ["scans/logo.png", "scans/nested/logo.PNG", "explicit.png"]
    );
    let planned_files = plan["files"].as_array().unwrap();
    assert_eq!(planned_files[0]["outputFilename"], "exports/logo.svg");
    assert_eq!(planned_files[1]["outputFilename"], "exports/logo_1.svg");
    assert!(
        planned_files
            .iter()
            .all(|file| file["status"] == "would-write")
    );
    assert!(!profiles.exists());
    assert_eq!(fs::read_dir(root.join("exports")).unwrap().count(), 0);

    args.pop();
    let run = invoke(root, &args);
    success(&run);
    let run: Value = serde_json::from_slice(&run.stdout).unwrap();
    assert_eq!(run["converted"], 3);
    for (actual, planned) in run["files"].as_array().unwrap().iter().zip(planned_files) {
        assert_eq!(actual["filename"], planned["filename"]);
        assert_eq!(actual["outputFilename"], planned["outputFilename"]);
        let output_path = root.join(actual["outputFilename"].as_str().unwrap());
        assert!(fs::read(output_path).unwrap().starts_with(b"<?xml"));
    }

    let profile_before = fs::read(&profiles).unwrap();
    let before = fs::read(root.join("exports/logo.svg")).unwrap();
    for (policy, exit_code, status) in [
        ("skip", 0, "skipped"),
        ("replace", 0, "would-write"),
        ("cancel", 130, "would-cancel"),
    ] {
        let run = invoke(
            root,
            &[
                "batch",
                "scans",
                "--recursive",
                "--format",
                "svg",
                "--output-dir",
                "exports",
                "--overwrite",
                policy,
                "--dry-run",
            ],
        );
        assert_eq!(run.status.code(), Some(exit_code));
        assert!(run.stderr.is_empty());
        let report: Value = serde_json::from_slice(&run.stdout).unwrap();
        assert!(
            report["files"]
                .as_array()
                .unwrap()
                .iter()
                .all(|file| file["status"] == status)
        );
    }
    assert_eq!(fs::read(&profiles).unwrap(), profile_before);
    assert_eq!(fs::read(root.join("exports/logo.svg")).unwrap(), before);

    fs::create_dir(root.join("bad-scan")).unwrap();
    fs::write(root.join("bad-scan/bad.png"), b"invalid image").unwrap();
    let mut args = vec!["batch", "bad-scan", "--output-dir", "exports", "--dry-run"];
    let plan = invoke(root, &args);
    success(&plan);
    let plan: Value = serde_json::from_slice(&plan.stdout).unwrap();
    assert_eq!(plan["files"][0]["status"], "would-write");

    args.pop();
    let run = invoke(root, &args);
    assert_eq!(run.status.code(), Some(1));
    let report: Value = serde_json::from_slice(&run.stdout).unwrap();
    assert_eq!(report["files"][0]["status"], "failed-load");
    assert!(!root.join("exports/bad.ai").exists());

    fs::create_dir(root.join("empty")).unwrap();
    failure(
        &invoke(root, &["batch", "empty", "--output-dir", "exports"]),
        "No supported images",
    );
    failure(
        &invoke(root, &["batch", "scans", "--output-dir", "explicit.png"]),
        "existing folder",
    );
}

#[test]
fn forced_vector_export_replaces_only_the_destination_entry() {
    use std::fs;

    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    fs::write(root.join("input.png"), IMAGE).unwrap();
    fs::write(root.join("keep"), b"original contents").unwrap();
    fs::hard_link(root.join("keep"), root.join("result.svg")).unwrap();

    let expected = invoke(root, &["vectorize", "input.png"]);
    success(&expected);
    let run = invoke(
        root,
        &["vectorize", "input.png", "-o", "result.svg", "--force"],
    );
    success(&run);
    assert!(expected.stdout.starts_with(b"<?xml"));
    assert_eq!(fs::read(root.join("result.svg")).unwrap(), expected.stdout);
    assert_eq!(fs::read(root.join("keep")).unwrap(), b"original contents");

    fs::write(root.join("edits.json"), b"[]").unwrap();
    let run = invoke(
        root,
        &[
            "vectorize",
            "input.png",
            "--edits",
            "edits.json",
            "--format",
            "svg",
            "-o",
            "edits.json",
            "--force",
        ],
    );
    failure(&run, "Edit plan and output must be different files.");
    assert_eq!(fs::read(root.join("edits.json")).unwrap(), b"[]");

    #[cfg(unix)]
    {
        std::os::unix::fs::symlink("keep", root.join("linked.svg")).unwrap();
        let run = invoke(
            root,
            &["vectorize", "input.png", "-o", "linked.svg", "--force"],
        );
        failure(&run, "not a regular file");
        assert!(
            fs::symlink_metadata(root.join("linked.svg"))
                .unwrap()
                .is_symlink()
        );
        assert_eq!(fs::read(root.join("keep")).unwrap(), b"original contents");
    }
    assert!(!fs::read_dir(root).unwrap().any(|entry| {
        entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".openvector-")
    }));
}
